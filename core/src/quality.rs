//! 音质判定：从列表接口的 MINFO / 实际音频探测结果推断真实档位。
//!
//! 设计原则：**不信请求参数，只信文件头 + 字节数 + 时长**。
//! 服务端对受限曲会返回固定的 181,521 字节占位文件；只看 Content-Type
//! 或“请求成功”会把 11 秒片段当成整曲。

use crate::error::{Error, Result};
use crate::http::{HttpClient, ProbeInfo};
use crate::model::{AudioFormat, Codec, Quality, StreamInfo};

/// 酷我匿名受限请求固定下发的占位文件大小。
pub const PLACEHOLDER_BYTES: u64 = 181_521;
/// 占位文件 URL 里的固定路径片段。
pub const PLACEHOLDER_MARKER: &str = "588957081";

/// WebView（WKWebView）不认 `audio/x-flac`，必须发标准 MIME。
///
/// 实测 macOS 26 的 WKWebView：
///   canPlayType('audio/flac')   -> "maybe"
///   canPlayType('audio/x-flac') -> ""
/// 酷我 CDN 返回的是 `audio/x-flac`，所以代理层必须转换。
pub fn mime_for_codec(codec: Codec) -> &'static str {
    match codec {
        Codec::Flac => "audio/flac",
        Codec::Ogg => "audio/ogg",
        Codec::Aac | Codec::Alac => "audio/mp4",
        Codec::Wav => "audio/wav",
        Codec::Mp3 | Codec::Unknown => "audio/mpeg",
    }
}

pub fn normalize_content_type(ct: &str) -> String {
    let l = ct.trim().to_ascii_lowercase();
    if l.contains("flac") {
        "audio/flac".into()
    } else if l.contains("mpeg") || l.contains("mp3") {
        "audio/mpeg".into()
    } else if l.contains("ogg") {
        "audio/ogg".into()
    } else if l.contains("mp4") || l.contains("m4a") || l.contains("aac") {
        "audio/mp4".into()
    } else if l.contains("wav") || l.contains("wave") {
        "audio/wav".into()
    } else {
        ct.to_string()
    }
}

/// 解析搜索/歌单接口的 `MINFO`，得到当前可播放的档位。
///
/// 形如：
/// `level:ff,bitrate:2000,format:flac,size:24.81Mb;level:p,bitrate:320,format:mp3,...`
pub fn qualities_from_minfo(raw: &str) -> Vec<Quality> {
    let mut out = Vec::new();
    for item in raw.split(';') {
        let mut level = "";
        let mut bitrate = 0u32;
        let mut format = "";
        for kv in item.split(',') {
            let Some((k, v)) = kv.split_once(':') else {
                continue;
            };
            match k.trim() {
                "level" => level = v.trim(),
                "bitrate" => bitrate = v.trim().parse().unwrap_or(0),
                "format" => format = v.trim(),
                _ => {}
            }
        }
        let q = match (level, format, bitrate) {
            ("ff", "flac", _) => Some(Quality::Lossless),
            (_, "mp3", 256..=u32::MAX) => Some(Quality::Super),
            (_, "mp3", 1..=160) => Some(Quality::Standard),
            _ => None,
        };
        if let Some(q) = q {
            if !out.contains(&q) {
                out.push(q);
            }
        }
    }
    out.sort_by_key(|q| q.rank());
    out
}

/// 从 `FORMATS` 或 `N_MINFO` 判断目录里是否存在加密高级格式。
///
/// 必须按 token 匹配：`AAC48` 里含有 `AC4` 子串，不能直接 `contains`。
pub fn has_advanced_formats(raw: &str) -> bool {
    for token in raw.split(['|', ';', ',']) {
        let t = token.trim().to_ascii_uppercase();
        let t = t.strip_prefix("LEVEL:").unwrap_or(&t);
        let t = t.strip_prefix("FORMATS=").unwrap_or(t);
        let t = t.strip_prefix("N_MINFO=").unwrap_or(t);
        if t.starts_with("ZPGA")
            || t == "ZPLY"
            || t == "DTSX"
            || t == "BCMS"
            || t.starts_with("MNAC")
            || t.starts_with("DDJOC")
            || t.starts_with("AC4")
        {
            return true;
        }
    }
    false
}

/// 从 `FORMATS` / musicinfo 文本判断可播档位（musicinfo 的 S-lines）。
pub fn qualities_from_formats(raw: &str) -> Vec<Quality> {
    let mut out = Vec::new();
    if raw.contains("ALFLAC") || raw.contains("ALFLAC=") {
        out.push(Quality::Lossless);
    }
    if raw.contains("MP3H") {
        out.push(Quality::Super);
    }
    if raw.contains("MP3128") {
        out.push(Quality::Standard);
    }
    out.sort_by_key(|q| q.rank());
    out
}

/// 探测一个直链并校验，返回带实际格式的 `StreamInfo`。
///
/// `requested` 只用于降级标记；实际档位以文件头/码率为准。
pub async fn verify_url(
    http: &HttpClient,
    url: &str,
    duration_sec: u32,
    requested: Quality,
    via: &str,
) -> Result<StreamInfo> {
    if requested.is_advanced() {
        return Err(Error::Unsupported(format!(
            "{} 需要会员账号与加密音频解密，当前未接入",
            requested.label()
        )));
    }

    let probe = http.probe_stream(url).await.unwrap_or_default();
    verify_probe(&probe, url, duration_sec, requested, via)
}

/// 同 `verify_url`，但探测结果已由调用方拿到（便于单测）。
pub fn verify_probe(
    probe: &ProbeInfo,
    url: &str,
    duration_sec: u32,
    requested: Quality,
    via: &str,
) -> Result<StreamInfo> {
    if !(probe.status == 200 || probe.status == 206) {
        return Err(Error::Http(format!("音频探测状态码 {}", probe.status)));
    }
    if url.contains(PLACEHOLDER_MARKER)
        || probe.bytes == Some(PLACEHOLDER_BYTES)
        || probe.bytes == Some(0)
    {
        return Err(Error::Unavailable);
    }
    let ct = probe.content_type.to_ascii_lowercase();
    if ct.contains("text/html") || ct.contains("application/json") {
        return Err(Error::Unavailable);
    }

    let format = AudioFormat::from_probe(probe, duration_sec);
    let actual = format.tier().unwrap_or(requested);

    // 片段判定：字节数按标称时长折算出的平均码率过低，说明只拿到了片段。
    // 固定占位文件/URL 已在上面拦截；这里兜住“短片段但 Content-Type 正常”的情况。
    if duration_sec >= 30 {
        if let Some(kbps) = format.bitrate_kbps {
            if kbps < 48 {
                return Err(Error::Unavailable);
            }
            if let Some(want) = requested.nominal_kbps() {
                // 请求 320 却只给 128 属于可接受的降级（40%），
                // 低于 35% 则更像片段而不是整曲。
                if kbps.saturating_mul(100) < want.saturating_mul(35) {
                    return Err(Error::Unavailable);
                }
            }
        }
    }

    // 无损必须由文件头确认；只有 Content-Type 时保留 FLAC 编码但标未校验。
    let verified = format.verified;

    let quality = if format.lossless {
        Quality::Lossless
    } else {
        actual
    };
    let degraded = quality.rank() < requested.rank();

    Ok(StreamInfo {
        url: url.to_string(),
        quality,
        requested,
        bytes: probe.bytes,
        content_type: {
            let normalized = normalize_content_type(&probe.content_type);
            if normalized.starts_with("audio/") {
                normalized
            } else {
                mime_for_codec(format.codec).to_string()
            }
        },
        availability: crate::model::Availability::Available,
        format,
        via: via.to_string(),
        degraded,
        verified,
    })
}

impl AudioFormat {
    /// 从探测结果推断实际格式。
    pub fn from_probe(probe: &ProbeInfo, duration_sec: u32) -> AudioFormat {
        let head = probe.head.as_slice();
        let ct = probe.content_type.to_ascii_lowercase();
        let mut codec = Codec::Unknown;
        let mut verified = false;
        let mut sample_rate_hz = None;
        let mut bits_per_sample = None;
        let mut channels = None;

        if let Some((rate, bits, ch)) = parse_flac_streaminfo(head) {
            codec = Codec::Flac;
            verified = true;
            sample_rate_hz = Some(rate);
            bits_per_sample = Some(bits);
            channels = Some(ch);
        } else if head.starts_with(b"OggS") {
            codec = Codec::Ogg;
            verified = true;
        } else if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
            codec = Codec::Wav;
            verified = true;
        } else if looks_like_mp3(head) {
            codec = Codec::Mp3;
            verified = true;
        } else if ct.contains("flac") {
            codec = Codec::Flac;
        } else if ct.contains("ogg") {
            codec = Codec::Ogg;
        } else if ct.contains("aac") || ct.contains("mp4") || ct.contains("m4a") {
            codec = Codec::Aac;
        } else if ct.contains("mpeg") || ct.contains("mp3") {
            codec = Codec::Mp3;
        }

        // ID3 头前面的 FLAC 也能识别（部分 CDN 会在 FLAC 前塞 ID3）。
        if codec == Codec::Unknown {
            if let Some(offset) = id3_offset(head) {
                if head.get(offset..offset + 4) == Some(b"fLaC") {
                    codec = Codec::Flac;
                    verified = true;
                    if let Some((rate, bits, ch)) = parse_flac_streaminfo(&head[offset..]) {
                        sample_rate_hz = Some(rate);
                        bits_per_sample = Some(bits);
                        channels = Some(ch);
                    }
                }
            }
        }

        let bitrate_kbps = match (probe.bytes, duration_sec) {
            (Some(bytes), d) if d > 0 && bytes > 0 => {
                Some(((bytes as f64 * 8.0) / (d as f64 * 1000.0)).round() as u32)
            }
            _ => None,
        };

        AudioFormat {
            codec,
            bitrate_kbps,
            sample_rate_hz,
            bits_per_sample,
            channels,
            lossless: codec.is_lossless() && verified,
            verified,
        }
    }
}

/// 解析 FLAC 的 STREAMINFO（采样率/位深/声道）。
fn parse_flac_streaminfo(head: &[u8]) -> Option<(u32, u8, u8)> {
    let offset = head.windows(4).position(|w| w == b"fLaC")?;
    let p = offset + 4;
    let block = head.get(p..p + 4)?;
    // 只处理 STREAMINFO（block type 0），长度必须 34
    if block[0] & 0x7f != 0 {
        return None;
    }
    let len = ((block[1] as usize) << 16) | ((block[2] as usize) << 8) | block[3] as usize;
    if len < 34 {
        return None;
    }
    let si = head.get(p + 4..p + 4 + 18)?;
    let packed = u64::from_be_bytes(si[10..18].try_into().ok()?);
    let rate = ((packed >> 44) & 0xfffff) as u32;
    let channels = (((packed >> 41) & 0x7) + 1) as u8;
    let bits = (((packed >> 36) & 0x1f) + 1) as u8;
    if rate == 0 {
        return None;
    }
    Some((rate, bits, channels))
}

fn id3_offset(head: &[u8]) -> Option<usize> {
    if head.len() < 10 || &head[..3] != b"ID3" {
        return None;
    }
    let size = ((head[6] as usize & 0x7f) << 21)
        | ((head[7] as usize & 0x7f) << 14)
        | ((head[8] as usize & 0x7f) << 7)
        | (head[9] as usize & 0x7f);
    Some(10 + size)
}

fn looks_like_mp3(head: &[u8]) -> bool {
    if head.starts_with(b"ID3") {
        return true;
    }
    head.len() >= 2 && head[0] == 0xff && (head[1] & 0xe0) == 0xe0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::ProbeInfo;

    fn probe(head: &[u8], bytes: u64, ct: &str) -> ProbeInfo {
        ProbeInfo {
            status: 206,
            bytes: Some(bytes),
            content_type: ct.into(),
            head: head.to_vec(),
        }
    }

    #[test]
    fn parses_minfo() {
        let raw = "level:ff,bitrate:2000,format:flac,size:24.81Mb;level:p,bitrate:320,format:mp3,size:8.53Mb;level:h,bitrate:128,format:mp3,size:3.41Mb";
        assert_eq!(
            qualities_from_minfo(raw),
            vec![Quality::Standard, Quality::Super, Quality::Lossless]
        );
    }

    #[test]
    fn normalizes_webview_mime() {
        assert_eq!(normalize_content_type("audio/x-flac"), "audio/flac");
        assert_eq!(normalize_content_type("audio/flac"), "audio/flac");
        assert_eq!(normalize_content_type("audio/x-mpeg"), "audio/mpeg");
        assert_eq!(normalize_content_type("audio/ogg"), "audio/ogg");
        assert_eq!(normalize_content_type("application/octet-stream"), "application/octet-stream");
    }

    #[test]
    fn detects_advanced_formats() {
        assert!(has_advanced_formats("ZPGA201|ZPLY|DTSX|BCMS|MNAC440"));
        assert!(has_advanced_formats(
            "FORMATS=MUSIC_440613|MNAC440|DTSX|AAC48"
        ));
        assert!(!has_advanced_formats("MP3H|ALFLAC|MP3128|AAC48"));
    }

    #[test]
    fn flac_streaminfo() {
        // 440613 稻香的真实前 64 字节
        let head = b"fLaC\x00\x00\x00\"\x10\x00\x10\x00\x00\t\xe9\x008N\n\xc4B\xf0\x00\x96]E\xaf\x11A\xbd\xe7\x04\x80\xff2\xc3S\n\x9f\xad\xf7'\x03\x00\x01\x9e\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x10\x00";
        let f = AudioFormat::from_probe(&probe(head, 26_012_257, "audio/x-flac"), 223);
        assert_eq!(f.codec, Codec::Flac);
        assert!(f.lossless);
        assert_eq!(f.sample_rate_hz, Some(44_100));
        assert_eq!(f.bits_per_sample, Some(16));
        assert_eq!(f.channels, Some(2));
    }

    #[test]
    fn rejects_placeholder_and_short() {
        let p = probe(b"", 181_521, "audio/mpeg");
        assert!(verify_probe(&p, "https://x/588957081.mp3", 223, Quality::Super, "anti").is_err());

        // 128k 文件冒充 320k：3.5MB / 223s ≈ 128k，估算时长 ~223s；
        // 按 320k 目标请求时，实际档位会被判为 standard，而不是失败。
        let p = probe(b"ID3", 3_576_668, "audio/mpeg");
        let info = verify_probe(&p, "https://x/a.mp3", 223, Quality::Super, "anti").unwrap();
        assert_eq!(info.quality, Quality::Standard);
        assert!(info.degraded);
    }
}
