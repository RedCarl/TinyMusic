//! 酷我资源管理协议的加解密。
//!
//! 来源：`lidx.dll` 中紧邻 `U_RES` / `res_manage` 字符串的一段例程
//! （VA 0x10008650，密钥常量位于 0x1000f564 / 0x1000f574）。
//!
//! 算法是「两轮 旋转 + 定长异或」，没有 S 盒，也没有分组填充：
//!
//! ```text
//! 加密: rotate_right(133) → xor(K13) → rotate_left(71) → xor(K23)
//! 解密: xor(K23) → rotate_right(71) → xor(K13) → rotate_left(133)
//! ```
//!
//! 密钥长度分别是 13 和 23 字节，异或是按 `i % key.len()` 循环取字节。
//! 旋转量对消息长度取模（消息为空时直接返回）。
//!
//! 正确性由 `tests::matches_real_client_ciphertext` 用真实抓包向量保证：
//! 加密结果与客户端线上字节完全一致。

/// 第一轮密钥（13 字节）
const K13: &[u8; 13] = b"KoOtOiTvINGwd";
/// 第二轮密钥（23 字节）
const K23: &[u8; 23] = b"_Y8g2E6n0E1i7L5t2IoOoNk";

fn rotate_left_in_place(buf: &mut [u8], n: usize) {
    if buf.is_empty() {
        return;
    }
    buf.rotate_left(n % buf.len());
}

fn rotate_right_in_place(buf: &mut [u8], n: usize) {
    if buf.is_empty() {
        return;
    }
    buf.rotate_right(n % buf.len());
}

fn xor_key(buf: &mut [u8], key: &[u8]) {
    if key.is_empty() {
        return;
    }
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

/// 明文 → 线上密文
pub fn encrypt(plain: &[u8]) -> Vec<u8> {
    let mut x = plain.to_vec();
    rotate_right_in_place(&mut x, 133);
    xor_key(&mut x, K13);
    rotate_left_in_place(&mut x, 71);
    xor_key(&mut x, K23);
    x
}

/// 线上密文 → 明文
pub fn decrypt(cipher: &[u8]) -> Vec<u8> {
    let mut x = cipher.to_vec();
    xor_key(&mut x, K23);
    rotate_right_in_place(&mut x, 71);
    xor_key(&mut x, K13);
    rotate_left_in_place(&mut x, 133);
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// 真实抓包向量：客户端发出的明文体 + 线上密文体
    const PLAIN: &[u8] = b"<001><U_QRY>|<1468492805,3166832472>|<58365624><MUSIC_9.6.0.0_W1><kwmusic_web_1_bds_20171222.exe>|<>|<rid>|<uip:0.0.0.0>|<new>|<nat:3>|<flags:262144><speer>|<ipdeny:no>|<loginid:645793966>\r\n";
    const CIPHER_B64: &str = "ZU4FE0YMLhk5ZiRBLSJxC04/LB8aHVdbczRBenBEQDdFH2gBAhRyWEMabUdeZnpfEylLGFcvQDhPKGU5VzF6NjYqJztwR38OdWN2fhBtJ38UQDl6IEwKEwoWEQcaIkIXdx8BPUoJTTZWDE4OSwJGUVNIXE8JQidXGlcqIHtUIUJpBW4jcWljRHZnaXBAdhdWIXptOnQRbypxD1JrYE4aXRgVam1BLWhGVXtUE3ZCCFQtSA8IMRZFM3pTAC0BEA==";

    fn cipher() -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(CIPHER_B64)
            .expect("向量 base64 应能解开")
    }

    #[test]
    fn matches_real_client_ciphertext() {
        let got = encrypt(PLAIN);
        let want = cipher();
        assert_eq!(got.len(), want.len(), "密文长度应一致");
        assert_eq!(got, want, "加密结果必须与真实客户端字节级一致");
    }

    #[test]
    fn decrypts_real_response_prefix() {
        // 只验证「解密是加密的逆运算」在真实向量上成立
        let back = decrypt(&cipher());
        assert_eq!(back, PLAIN.to_vec());
    }

    #[test]
    fn roundtrip_various_lengths() {
        for n in 1..300usize {
            let data: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let enc = encrypt(&data);
            assert_eq!(enc.len(), data.len(), "长度必须保持不变");
            assert_eq!(decrypt(&enc), data, "n={} 往返失败", n);
        }
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(encrypt(&[]).is_empty());
        assert!(decrypt(&[]).is_empty());
    }
}
