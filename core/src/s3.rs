//! 极简 S3 客户端：SigV4 签名 + PUT Object + ListObjectsV2。
//!
//! 不引 aws-sdk（体积太大），只做网盘下载需要的三件事：
//! 上传对象、列对象、连通性探测。SHA-256 / HMAC-SHA256 用 `sha2` / `hmac`。
//!
//! 兼容 AWS S3、MinIO、Cloudflare R2、阿里云 OSS 的 S3 兼容端点等。
//! Path-Style 与 Virtual-Hosted-Style 两种寻址都支持。

use crate::error::{Error, Result};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 网盘配置（全部来自设置页）
#[derive(Debug, Clone, Default)]
pub struct S3Config {
    /// 形如 `https://s3.us-east-1.amazonaws.com` 或 `http://127.0.0.1:9000`
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// 桶内前缀目录，可为空
    pub prefix: String,
    /// true 时用 `endpoint/bucket/key`，false 时用 `bucket.endpoint/key`
    pub path_style: bool,
}

impl S3Config {
    pub fn is_ready(&self) -> bool {
        !self.endpoint.trim().is_empty()
            && !self.bucket.trim().is_empty()
            && !self.access_key.trim().is_empty()
            && !self.secret_key.trim().is_empty()
    }

    fn region_or_default(&self) -> &str {
        let r = self.region.trim();
        if r.is_empty() {
            "us-east-1"
        } else {
            r
        }
    }
}

struct Endpoint {
    scheme: String,
    host: String,
    port: Option<u16>,
    base_path: String,
}

impl Endpoint {
    fn parse(endpoint: &str) -> Result<Self> {
        let raw = if endpoint.contains("://") {
            endpoint.to_string()
        } else {
            format!("https://{}", endpoint)
        };
        let url = url::Url::parse(&raw)
            .map_err(|e| Error::Http(format!("S3 Endpoint 不合法：{}", e)))?;
        let scheme = url.scheme().to_string();
        if scheme != "http" && scheme != "https" {
            return Err(Error::Http("S3 Endpoint 只支持 http/https".into()));
        }
        let host = url
            .host_str()
            .ok_or_else(|| Error::Http("S3 Endpoint 缺少主机名".into()))?
            .to_string();
        Ok(Self {
            scheme,
            host,
            port: url.port(),
            base_path: url.path().trim_end_matches('/').to_string(),
        })
    }

    fn authority(&self, host_override: Option<&str>) -> String {
        let host = host_override.unwrap_or(&self.host);
        match self.port {
            Some(p) => format!("{}:{}", host, p),
            None => host.to_string(),
        }
    }
}

pub struct S3 {
    cfg: S3Config,
    http: reqwest::Client,
}

impl S3 {
    pub fn new(cfg: S3Config, http: reqwest::Client) -> Self {
        Self { cfg, http }
    }

    pub fn config(&self) -> &S3Config {
        &self.cfg
    }

    fn endpoint(&self) -> Result<Endpoint> {
        Endpoint::parse(self.cfg.endpoint.trim())
    }

    /// 请求真正使用的 Host 头（path-style 不带 bucket，virtual 带前缀）
    fn host_header(&self) -> Result<String> {
        let ep = self.endpoint()?;
        let bucket = self.cfg.bucket.trim().trim_matches('/');
        if self.cfg.path_style {
            Ok(ep.authority(None))
        } else {
            Ok(ep.authority(Some(&format!("{}.{}", bucket, ep.host))))
        }
    }

    /// 把 `key` 拼成完整的规范路径（已做 AWS 风格 percent-encoding）
    fn object_path(&self, key: &str) -> Result<(String, String)> {
        let ep = self.endpoint()?;
        let bucket = self.cfg.bucket.trim().trim_matches('/');
        let key = key.trim_start_matches('/');
        let mut path = ep.base_path.clone();
        if self.cfg.path_style {
            path.push('/');
            path.push_str(bucket);
        }
        path.push('/');
        path.push_str(&uri_encode(key, false));
        let host = self.host_header()?;
        let url = format!("{}://{}{}", ep.scheme, host, path);
        Ok((url, path))
    }

    /// 写入一个对象
    pub async fn put_object(&self, key: &str, body: Vec<u8>, content_type: &str) -> Result<()> {
        let (url, path) = self.object_path(key)?;
        let payload_hash = sha256_hex(&body);
        let headers = self.sign("PUT", &path, "", &payload_hash, &[])?;

        let mut req = self
            .http
            .put(&url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(reqwest::header::CONTENT_LENGTH, body.len().to_string());
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .body(body)
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 上传请求失败：{}", e)))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!("S3 上传失败（HTTP {}）：{}", status, brief)));
        }
        Ok(())
    }

    /// 读取对象内容；对象不存在时返回 `Ok(None)`（比如还没同步过 favorites.json）
    pub async fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let (url, path) = self.object_path(key)?;
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("GET", &path, "", &empty_hash, &[])?;

        let mut req = self.http.get(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 读取请求失败：{}", e)))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        if !status.is_success() {
            let code = status.as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!("S3 读取失败（HTTP {}）：{}", code, brief)));
        }
        Ok(Some(resp.bytes().await?.to_vec()))
    }

    /// 列出前缀下的对象 key（最多 1000 个）
    pub async fn list_keys(&self, prefix: &str) -> Result<Vec<String>> {
        let (url, raw_path) = self.object_path("")?;
        let base = url.trim_end_matches('/');
        // 列表请求打的是「桶根」，路径末尾不能带 /，否则规范请求与实际请求对不上签名
        let path = {
            let p = raw_path.trim_end_matches('/');
            if p.is_empty() {
                "/".to_string()
            } else {
                p.to_string()
            }
        };
        let mut query: Vec<(&str, &str)> = vec![("list-type", "2"), ("max-keys", "1000")];
        if !prefix.is_empty() {
            query.push(("prefix", prefix));
        }
        query.sort_by(|a, b| a.0.cmp(b.0));
        // 注意：查询串里的 `/` 必须编码成 %2F。路径里的 `/` 才需要保留 ——
        // 两者用不同的规则，否则服务端按自己的规则重算签名会对不上，
        // 表现为「前缀里 case 一出现中文/斜杠就 SignatureDoesNotMatch」。
        let canonical_query = query
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
            .collect::<Vec<_>>()
            .join("&");
        let url = format!("{}?{}", base, canonical_query);
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("GET", &path, &canonical_query, &empty_hash, &[])?;

        let mut req = self.http.get(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 列表请求失败：{}", e)))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!("S3 列表失败（HTTP {}）：{}", status, brief)));
        }
        let body = resp.text().await?;
        Ok(parse_keys(&body))
    }

    /// 连通性测试：能列出对象就算通过
    pub async fn test(&self) -> Result<String> {
        let prefix = self.normalized_prefix();
        let keys = self.list_keys(&prefix).await?;
        let bucket = self.cfg.bucket.trim();
        Ok(format!(
            "连接成功：bucket「{}」可访问，前缀「{}」下已有 {} 个对象",
            bucket,
            if prefix.is_empty() { "/" } else { &prefix },
            keys.len()
        ))
    }

    pub fn normalized_prefix(&self) -> String {
        self.cfg.prefix.trim().trim_matches('/').to_string()
    }

    /// 生成 SigV4 头：x-amz-date / x-amz-content-sha256 / authorization
    fn sign(
        &self,
        method: &str,
        canonical_uri: &str,
        canonical_query: &str,
        payload_hash: &str,
        extra: &[(&str, String)],
    ) -> Result<Vec<(String, String)>> {
        let host = self.host_header()?;
        let (amz_date, date_stamp) = utc_now_parts();
        let region = self.cfg.region_or_default();

        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), host),
            ("x-amz-content-sha256".into(), payload_hash.into()),
            ("x-amz-date".into(), amz_date.clone()),
        ];
        for (k, v) in extra {
            headers.push((k.to_ascii_lowercase(), v.clone()));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_headers: String = headers
            .iter()
            .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
            .collect();
        let signed_headers = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");

        let canonical_request = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method, canonical_uri, canonical_query, canonical_headers, signed_headers, payload_hash
        );
        let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            scope,
            sha256_hex(canonical_request.as_bytes())
        );

        let k_date = hmac_sha256(
            format!("AWS4{}", self.cfg.secret_key.trim()).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
            self.cfg.access_key.trim(),
            scope,
            signed_headers,
            signature
        );

        let mut out = vec![
            ("x-amz-date".to_string(), amz_date),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("authorization".to_string(), authorization),
        ];
        for (k, v) in extra {
            out.push((k.to_string(), v.clone()));
        }
        Ok(out)
    }

    /// 删除对象（歌单删除/重命名时用）
    pub async fn delete_object(&self, key: &str) -> Result<()> {
        let (url, path) = self.object_path(key)?;
        let empty_hash = sha256_hex(&[]);
        let headers = self.sign("DELETE", &path, "", &empty_hash, &[])?;

        let mut req = self.http.delete(&url);
        for (k, v) in &headers {
            req = req.header(k.as_str(), v);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Http(format!("S3 删除请求失败：{}", e)))?;
        if !resp.status().is_success() && resp.status().as_u16() != 204 {
            let code = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let brief: String = text.chars().take(240).collect();
            return Err(Error::Http(format!("S3 删除失败（HTTP {}）：{}", code, brief)));
        }
        Ok(())
    }

    /// 移动对象：读 → 写新 key → 删旧 key。
    ///
    /// 没有用服务端 CopyObject：实测 RustFS 对带 `x-amz-copy-source` 的请求
    /// 签名处理与 AWS 不一致（SignatureDoesNotMatch），而 GET/PUT/DELETE 都是通的。
    pub async fn move_object(&self, from_key: &str, to_key: &str) -> Result<()> {
        let data = self
            .get_object(from_key)
            .await?
            .ok_or_else(|| Error::Http("源对象不存在".into()))?;
        self.put_object(to_key, data, content_type_for_key(to_key)).await?;
        self.delete_object(from_key).await
    }

    /// 生成带签名的 GET 直链（给 <audio>/<img> 直接用，默认 6 小时）
    pub fn presign_get(&self, key: &str, expires_secs: u64) -> Result<String> {
        let (url, path) = self.object_path(key)?;
        let host = self.host_header()?;
        let (amz_date, date_stamp) = utc_now_parts();
        let region = self.cfg.region_or_default();
        let scope = format!("{}/{}/s3/aws4_request", date_stamp, region);
        let expires = expires_secs.clamp(60, 604_800);

        let mut params: Vec<(&str, String)> = vec![
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_string()),
            (
                "X-Amz-Credential",
                format!("{}/{}", self.cfg.access_key.trim(), scope),
            ),
            ("X-Amz-Date", amz_date.clone()),
            ("X-Amz-Expires", expires.to_string()),
            ("X-Amz-SignedHeaders", "host".to_string()),
        ];
        params.sort_by(|a, b| a.0.cmp(b.0));
        let canonical_query = params
            .iter()
            .map(|(k, v)| format!("{}={}", uri_encode(k, true), uri_encode(v, true)))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_headers = format!("host:{}\n", host);
        let canonical_request = format!(
            "GET\n{}\n{}\n{}\nhost\nUNSIGNED-PAYLOAD",
            path, canonical_query, canonical_headers
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            amz_date,
            scope,
            sha256_hex(canonical_request.as_bytes())
        );
        let k_date = hmac_sha256(
            format!("AWS4{}", self.cfg.secret_key.trim()).as_bytes(),
            date_stamp.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, region.as_bytes());
        let k_service = hmac_sha256(&k_region, b"s3");
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        Ok(format!(
            "{}?{}&X-Amz-Signature={}",
            url, canonical_query, signature
        ))
    }

}

fn content_type_for_key(key: &str) -> &'static str {
    let lower = key.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".flac") {
        "audio/flac"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".lrc") || lower.ends_with(".txt") {
        "text/plain; charset=utf-8"
    } else if lower.ends_with(".json") {
        "application/json; charset=utf-8"
    } else if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// AWS 风格 percent-encoding：只保留 unreserved 字符（`/` 按需保留）
fn uri_encode(input: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for &b in input.as_bytes() {
        let unreserved = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~');
        if unreserved || (!encode_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex(&h.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度密钥");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Unix 秒 → (20231114T221320Z, 20231114)，UTC
fn utc_now_parts() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let date_stamp = format!("{:04}{:02}{:02}", y, m, d);
    let amz_date = format!("{}T{:02}{:02}{:02}Z", date_stamp, hh, mm, ss);
    (amz_date, date_stamp)
}

/// Howard Hinnant 的 days_from_civil 逆运算
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 从 ListObjectsV2 的 XML 里抠出 `<Key>`
fn parse_keys(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<Key>") {
        let after = &rest[start + 5..];
        let Some(end) = after.find("</Key>") else { break };
        out.push(xml_unescape(&after[..end]));
        rest = &after[end + 6..];
    }
    out
}

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_chars() {
        assert_eq!(uri_encode("a b/中文.mp3", false), "a%20b/%E4%B8%AD%E6%96%87.mp3");
        assert_eq!(uri_encode("a/b", true), "a%2Fb");
        // 查询串里的斜杠必须编码，路径里必须保留
        assert_eq!(
            uri_encode("TinyMusic/测试歌单/", true),
            "TinyMusic%2F%E6%B5%8B%E8%AF%95%E6%AD%8C%E5%8D%95%2F"
        );
    }

    #[test]
    fn formats_utc_timestamp() {
        // 1700000000 = 2023-11-14 22:13:20 UTC
        let days = 1_700_000_000i64.div_euclid(86_400);
        let rem = 1_700_000_000i64.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        let stamp = format!("{:04}{:02}{:02}", y, m, d);
        assert_eq!(stamp, "20231114");
        assert_eq!((rem / 3600, (rem % 3600) / 60, rem % 60), (22, 13, 20));
    }

    #[test]
    fn parses_listing_xml() {
        let xml = r#"<ListBucketResult><Contents><Key>a &amp; b.mp3</Key></Contents>
<Contents><Key>TinyMusic/歌单/x.flac</Key></Contents></ListBucketResult>"#;
        let keys = parse_keys(xml);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0], "a & b.mp3");
        assert_eq!(keys[1], "TinyMusic/歌单/x.flac");
    }

    #[test]
    fn builds_path_style_url() {
        let cfg = S3Config {
            endpoint: "http://127.0.0.1:9000".into(),
            region: "us-east-1".into(),
            bucket: "music".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            prefix: "TinyMusic".into(),
            path_style: true,
        };
        let s3 = S3::new(cfg, reqwest::Client::new());
        let (url, path) = s3.object_path("TinyMusic/歌单/a b.mp3").unwrap();
        assert_eq!(url, "http://127.0.0.1:9000/music/TinyMusic/%E6%AD%8C%E5%8D%95/a%20b.mp3");
        assert_eq!(path, "/music/TinyMusic/%E6%AD%8C%E5%8D%95/a%20b.mp3");
    }

    #[test]
    fn builds_virtual_host_style_url() {
        let cfg = S3Config {
            endpoint: "https://s3.us-east-1.amazonaws.com".into(),
            region: "us-east-1".into(),
            bucket: "music".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            prefix: String::new(),
            path_style: false,
        };
        let s3 = S3::new(cfg, reqwest::Client::new());
        let (url, _) = s3.object_path("a.mp3").unwrap();
        assert_eq!(url, "https://music.s3.us-east-1.amazonaws.com/a.mp3");
    }
}
