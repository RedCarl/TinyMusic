//! 网易云 `weapi` 协议层：参数加密 + cookie 会话。
//!
//! ## 为什么需要它
//!
//! 网易云网页端的所有私有接口（登录、每日推荐、歌单、取流）都走 `weapi`：
//!
//! ```text
//! POST https://music.163.com/weapi/{path}?csrf_token={csrf}
//! Content-Type: application/x-www-form-urlencoded
//! Cookie: MUSIC_U=...; __csrf=...
//!
//! params=<base64>&encSecKey=<hex>
//! ```
//!
//! * `params` = `AES-128-CBC( AES-128-CBC(json, NONCE), secretKey )`，base64
//! * `encSecKey` = `RSA_raw(reverse(secretKey), e=65537, n=MODULUS)`，hex（256 位）
//! * `secretKey` 是每次请求新生成的 16 位随机串
//!
//! 注意两个加密的**输入不同**：内层加密的是 JSON 文本，外层加密的是内层输出的
//! **base64 字符串本身**（不是二进制）。这一点弄错会得到服务端 `code=400`。
//!
//! ## 实测结论（2026-11-10，本机网络）
//!
//! | 接口 | 匿名 | 说明 |
//! |---|---|---|
//! | `v6/playlist/detail` | ✅ | 返回 `trackIds`（完整 id 列表），曲目详情需再查 `song/detail` |
//! | `song/detail` | ✅ | 批量 `ids`，免登录 |
//! | `v1/album/{id}` | ✅ | 专辑详情 |
//! | `v1/artist/{id}`、`artist/top/song` | ✅ | 艺人页 |
//! | `song/lyric` | ✅ | 歌词 |
//! | `cloudsearch/pc` | ✅ | `type=1` 单曲、`type=1000` 歌单 |
//! | `song/enhance/player/url` | ⚠️ | 免费曲 200，`fee=1` 的会员曲返回 `code=-110` |
//! | `v2/discovery/recommend/songs` | ❌ 301 | **必须登录** |
//! | `v1/discovery/recommend/resource` | ❌ 301 | **必须登录** |
//! | `login/qrcode/unikey` | ✅ | 返回 unikey（扫码登录第一步） |
//! | `login/cellphone` | ✅ | 账号密码错误时返回 `code=502`，说明加密与路径都正确 |

use crate::error::{Error, Result};
use crate::http::HttpClient;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use num_bigint::BigUint;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// 网易云 weapi 的固定 RSA 模数（1024 bit，页面 `core.js` 里写死）
const MODULUS: &str = "00e0b509f6259df8642dbc35662901477df22677ec152b5ff68ace615bb7b725152b3ab17a876aea8a5aa76d2e417629ec4ee341f56135fccf695280104e0312ecbda92557c93870114af6c9d05c4f7f0c3685b7a46bee255932575cce10b424d813cfe4875d3e82047b97ddef52741d546b8e289dc6935b3ece0462db0a22b8e7";
const PUBKEY: u64 = 0x010001;
/// 第一层 AES 的固定 key
const NONCE: &[u8; 16] = b"0CoJUm6Qyw8W8jud";
const AES_IV: &[u8; 16] = b"0102030405060708";
/// secretKey 的字符集
const BASE62: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

pub const WEB_HOST: &str = "music.163.com";
const WEB_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

/// `music.163.com/weapi/{path}`
fn weapi_url(path: &str) -> String {
    format!("https://{}/weapi/{}", WEB_HOST, path.trim_start_matches('/'))
}

// ---------- 加密 ----------

/// AES-128-CBC + PKCS#7（网易网页端用 CryptoJS 的 Pkcs7，与标准一致）
pub fn aes_cbc_encrypt(plain: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let cipher = Aes128::new(key.into());
    let mut buf = plain.to_vec();
    let pad = 16 - buf.len() % 16;
    buf.resize(buf.len() + pad, pad as u8);

    let mut prev = *AES_IV;
    for chunk in buf.chunks_mut(16) {
        for i in 0..16 {
            chunk[i] ^= prev[i];
        }
        let mut block = aes::Block::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        chunk.copy_from_slice(&block);
        prev.copy_from_slice(chunk);
    }
    buf
}

/// 仅用于自检/调试：把 `params` 解回 JSON（不参与线上请求）
pub fn aes_cbc_decrypt_pkcs7(cipher_text: &[u8], key: &[u8; 16]) -> Option<Vec<u8>> {
    use aes::cipher::BlockDecrypt;
    if cipher_text.is_empty() || cipher_text.len() % 16 != 0 {
        return None;
    }
    let cipher = Aes128::new(key.into());
    let mut out = Vec::with_capacity(cipher_text.len());
    let mut prev = *AES_IV;
    for chunk in cipher_text.chunks(16) {
        let mut block = aes::Block::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        for i in 0..16 {
            out.push(block[i] ^ prev[i]);
        }
        prev.copy_from_slice(chunk);
    }
    let pad = *out.last()? as usize;
    if pad == 0 || pad > 16 || pad > out.len() {
        return None;
    }
    out.truncate(out.len() - pad);
    Some(out)
}

/// 教科书 RSA（无填充）：网易只加密 16 字节的随机串，不需要 OAEP/PKCS#1。
pub fn rsa_encrypt_hex(text: &str) -> String {
    let reversed: String = text.chars().rev().collect();
    let n = BigUint::parse_bytes(MODULUS.as_bytes(), 16).expect("模数解析失败");
    let m = BigUint::from_bytes_be(reversed.as_bytes());

    // e = 65537 = 2^16 + 1：平方 16 次再乘一次。显式写出来是为了让
    // 「这里没做填充、也不做 hash」这件事在代码里可见。
    // 若将来 PUBKEY 被改动，下面的常量断言会直接编译失败。
    const _: () = assert!(PUBKEY == 0x1_0001);
    let mut c = m.clone();
    for _ in 0..16 {
        c = (&c * &c) % &n;
    }
    c = (c * &m) % &n;
    format!("{:0>256}", c.to_str_radix(16))
}

/// 生成 16 字节随机量，再映射到 base62 字符集。
///
/// 用 `RandomState`（标准库内部由 OS 熵播种的 SipHash）取随机字节，
/// 避免为了一个随机数引 `rand` 整棵树。
fn random_secret_key() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let mut raw = [0u8; 16];
    for (idx, slot) in raw.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        h.write_u128(nanos);
        h.write_usize(idx);
        slot.copy_from_slice(&h.finish().to_le_bytes());
    }

    let mut out = [0u8; 16];
    for (i, b) in raw.iter().enumerate() {
        out[i] = BASE62[(*b as usize) % BASE62.len()];
    }
    out
}

/// 生成请求体：`params=...&encSecKey=...`
pub fn encrypted_form(payload: &Value) -> String {
    let text = serde_json::to_string(payload).unwrap_or_else(|_| "{}".into());

    // 第一层：用固定 nonce 加密 JSON 文本
    let inner = BASE64.encode(aes_cbc_encrypt(text.as_bytes(), NONCE));

    // 第二层：用本次随机 key 加密「第一层的 base64 字符串」
    let key = random_secret_key();
    let params = BASE64.encode(aes_cbc_encrypt(inner.as_bytes(), &key));

    let secret = std::str::from_utf8(&key).unwrap_or("0123456789abcdef");
    let enc_sec_key = rsa_encrypt_hex(secret);

    let mut form = String::with_capacity(params.len() + 300);
    form.push_str("params=");
    form.push_str(&percent_encode(&params));
    form.push_str("&encSecKey=");
    form.push_str(&enc_sec_key);
    form
}

/// 只对 `params` 做百分号编码：base64 里的 `+ / =` 必须转义，
/// `encSecKey` 是纯 hex 不需要（和浏览器行为一致）。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ---------- 客户端 ----------

/// 带 cookie 罐的 weapi 客户端。
///
/// cookie 用 `std::sync::Mutex`：这些操作都是「取一个小 Vec 的副本」，
/// 不涉及 IO，用异步锁只会把 `set_session`（同步 trait 方法）逼进死路。
pub struct WeapiClient {
    http: Arc<HttpClient>,
    cookies: Mutex<Vec<(String, String)>>,
}

impl WeapiClient {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self {
            http,
            cookies: Mutex::new(Vec::new()),
        }
    }

    /// 用 `k=v; k2=v2` 形式的 cookie 串整体替换 cookie 罐。
    pub fn set_cookie_header(&self, raw: &str) {
        let mut jar = Vec::new();
        for part in raw.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                let k = k.trim().to_string();
                if k.is_empty() {
                    continue;
                }
                jar.retain(|(ek, _): &(String, String)| ek != &k);
                jar.push((k, v.trim().to_string()));
            }
        }
        if let Ok(mut guard) = self.cookies.lock() {
            *guard = jar;
        }
    }

    pub fn cookie(&self, name: &str) -> Option<String> {
        self.cookies
            .lock()
            .ok()?
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    }

    pub fn cookie_header(&self) -> String {
        self.cookies
            .lock()
            .map(|jar| {
                jar.iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    }

    pub fn clear_cookies(&self) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.clear();
        }
    }

    /// 是否持有登录票据
    pub fn has_login(&self) -> bool {
        self.cookie("MUSIC_U").map(|v| !v.is_empty()) == Some(true)
    }

    /// 合并响应里的 `Set-Cookie`（登录接口靠它下发 `MUSIC_U`）。
    fn absorb_set_cookie(&self, raw: &[String]) {
        let Ok(mut jar) = self.cookies.lock() else {
            return;
        };
        for line in raw {
            // 只需要第一段 `k=v`
            let Some(first) = line.split(';').next() else {
                continue;
            };
            let Some((k, v)) = first.split_once('=') else {
                continue;
            };
            let k = k.trim();
            if k.is_empty() {
                continue;
            }
            jar.retain(|(ek, _): &(String, String)| ek != k);
            jar.push((k.to_string(), v.trim().to_string()));
        }
    }

    /// 发一个 weapi 请求，返回已解析的 JSON。
    pub async fn post(&self, path: &str, payload: Value) -> Result<Value> {
        let body = encrypted_form(&payload);
        let cookie = self.cookie_header();
        let csrf = self.cookie("__csrf").unwrap_or_default();

        let mut url = weapi_url(path);
        if !csrf.is_empty() {
            url.push_str(if url.contains('?') { "&" } else { "?" });
            url.push_str("csrf_token=");
            url.push_str(&csrf);
        }

        let mut headers: Vec<(&str, &str)> = vec![
            ("referer", "https://music.163.com/"),
            ("origin", "https://music.163.com"),
            ("user-agent", WEB_UA),
            ("accept", "*/*"),
        ];
        if !cookie.is_empty() {
            headers.push(("cookie", &cookie));
        }

        let (text, set_cookie) = self
            .http
            .post_form_with(WEB_HOST, &url, &headers, &body)
            .await?;
        self.absorb_set_cookie(&set_cookie);

        serde_json::from_str(&text).map_err(|e| {
            let head: String = text.chars().take(160).collect();
            Error::Parse(format!("网易云响应不是 JSON: {e} | {head}"))
        })
    }

    /// GET 一个非 weapi 的 JSON 接口（`/api/...`）。
    pub async fn get(&self, path: &str) -> Result<Value> {
        let cookie = self.cookie_header();
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("https://{}/{}", WEB_HOST, path.trim_start_matches('/'))
        };
        let mut headers: Vec<(&str, &str)> =
            vec![("referer", "https://music.163.com/"), ("user-agent", WEB_UA)];
        if !cookie.is_empty() {
            headers.push(("cookie", &cookie));
        }
        let text = self.http.get_text_with(WEB_HOST, &url, &headers).await?;
        serde_json::from_str(&text).map_err(|e| {
            let head: String = text.chars().take(160).collect();
            Error::Parse(format!("网易云响应不是 JSON: {e} | {head}"))
        })
    }
}

/// 从响应里读 `code`，缺省视为 200。
pub fn code_of(v: &Value) -> i64 {
    v.get("code").and_then(|c| c.as_i64()).unwrap_or(200)
}

/// 把网易云的错误码翻译成给用户看的中文。
pub fn explain_code(code: i64) -> &'static str {
    match code {
        301 | 302 => "需要登录网易云后才能使用该接口",
        -110 => "该曲目无版权或需要会员，当前账号不可播放",
        -447 => "触发风控，请稍后再试",
        400 => "接口参数错误（协议可能已变更）",
        502 => "账号或密码错误",
        501 => "账号不存在",
        8821 => "需要安全验证（短信/设备校验），请改用网页登录或 cookie 导入",
        200 => "成功",
        _ => "接口返回异常",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn aes_round_trip() {
        let key = *b"abcdefghijklmnop";
        let plain = br#"{"s":"qingtian","type":1}"#;
        let ct = aes_cbc_encrypt(plain, &key);
        assert_eq!(ct.len() % 16, 0);
        let back = aes_cbc_decrypt_pkcs7(&ct, &key).unwrap();
        assert_eq!(back, plain.to_vec());
    }

    #[test]
    fn pkcs7_always_pads_full_block() {
        // 16 字节明文必须补出 16 字节填充，得到 32 字节密文；否则服务端解不出 JSON
        let key = *NONCE;
        let ct = aes_cbc_encrypt(b"0123456789abcdef", &key);
        assert_eq!(ct.len(), 32);
        assert_eq!(
            aes_cbc_decrypt_pkcs7(&ct, &key).unwrap(),
            b"0123456789abcdef".to_vec()
        );
    }

    #[test]
    fn rsa_shape() {
        let hex = rsa_encrypt_hex("0123456789abcdef");
        assert_eq!(hex.len(), 256, "encSecKey 必须是 256 位 hex");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn form_shape() {
        let form = encrypted_form(&json!({"s": "qingtian", "type": "1"}));
        assert!(form.starts_with("params="));
        assert!(form.contains("&encSecKey="));
        let params = form
            .trim_start_matches("params=")
            .split("&encSecKey=")
            .next()
            .unwrap();
        assert!(!params.contains('+') && !params.contains('/') && !params.contains('='));
    }

    #[test]
    fn each_request_uses_fresh_key() {
        let a = encrypted_form(&json!({"x": 1}));
        let b = encrypted_form(&json!({"x": 1}));
        assert_ne!(a, b, "每次请求的 secretKey 都应该是新的");
    }

    #[test]
    fn cookie_jar_round_trip() {
        let api = WeapiClient::new(Arc::new(HttpClient::new(0, 1)));
        api.set_cookie_header("MUSIC_U=abc; __csrf=def");
        assert!(api.has_login());
        assert_eq!(api.cookie("__csrf").as_deref(), Some("def"));
        assert_eq!(api.cookie_header(), "MUSIC_U=abc; __csrf=def");
        api.set_cookie_header("MUSIC_U=xyz");
        assert_eq!(api.cookie_header(), "MUSIC_U=xyz");
    }
}
