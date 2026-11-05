//! 下载引擎：流式落盘 → 校验 → 写 ID3 → 改名。
//!
//! 关键点：**先校验再认账**。若最终大小命中占位文件特征，直接判定失败并清理残文件，
//! 避免用户拿到一个 11 秒的「假歌」。

use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::id3;
use crate::model::{Quality, StreamInfo, Track};
use crate::providers::kuwo::PLACEHOLDER_BYTES;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

/// 进度回调：(已下载字节, 总字节)
pub type ProgressFn = Box<dyn Fn(u64, Option<u64>) + Send + Sync>;

pub struct Downloader {
    http: Arc<HttpClient>,
}

impl Downloader {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self { http }
    }

    /// 下载到 `dir/fname`，返回最终路径
    pub async fn run(
        &self,
        track: &Track,
        info: &StreamInfo,
        dir: &Path,
        fname: &str,
        on_progress: ProgressFn,
    ) -> Result<PathBuf> {
        tokio::fs::create_dir_all(dir).await?;
        let final_path = unique_path(dir, fname);
        let part_path = final_path.with_extension("part");

        let resp = self.http.get_range(&info.url, None).await?;
        if !resp.status().is_success() {
            return Err(Error::Http(format!("下载状态码 {}", resp.status().as_u16())));
        }
        let total = resp.content_length().or(info.bytes);

        let mut file = tokio::fs::File::create(&part_path).await?;
        let mut written: u64 = 0;
        let mut resp = resp;

        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
            on_progress(written, total);
        }
        file.flush().await?;
        drop(file);

        // 防护：占位文件不落盘
        if written == PLACEHOLDER_BYTES {
            let _ = tokio::fs::remove_file(&part_path).await;
            return Err(Error::Unavailable);
        }
        // 防护：明显短于标称时长（按档位码率估算）
        if track.duration_sec >= 30 && info.quality != Quality::Lossless {
            let est = (written as f64 * 8.0) / (info.quality.nominal_kbps() as f64 * 1000.0);
            if est * 100.0 < (track.duration_sec as f64) * 60.0 {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(Error::Unavailable);
            }
        }

        // 写标签后改名
        let tmp = part_path.clone();
        let title = track.title.clone();
        let artist = track.artist.clone();
        let album = track.album.clone();
        tokio::task::spawn_blocking(move || {
            id3::write_tags(&tmp, &title, &artist, &album).map_err(Error::Io)
        })
        .await
        .map_err(|e| Error::Http(format!("标签写入线程失败: {e}")))??;

        tokio::fs::rename(&part_path, &final_path).await?;
        on_progress(written, Some(written));
        Ok(final_path)
    }
}

/// 文件名清洗：去掉 Windows 非法字符，限长 120 字符
pub fn sanitize_filename(s: &str) -> String {
    let bad = ['\\', '/', ':', '*', '?', '"', '<', '>', '|', '\r', '\n', '\t'];
    let mut out: String = s
        .chars()
        .map(|c| if bad.contains(&c) { '_' } else { c })
        .collect();
    // 合并连续空白
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out = out.trim().to_string();
    if out.chars().count() > 120 {
        out = out.chars().take(120).collect();
    }
    if out.is_empty() {
        out = "unknown".into();
    }
    out
}

pub fn build_filename(track: &Track, ext: &str) -> String {
    let base = if track.artist.is_empty() {
        sanitize_filename(&track.title)
    } else {
        format!(
            "{} - {}",
            sanitize_filename(&track.artist),
            sanitize_filename(&track.title)
        )
    };
    format!("{}.{}", base, ext)
}

fn unique_path(dir: &Path, fname: &str) -> PathBuf {
    let mut p = dir.join(fname);
    if !p.exists() {
        return p;
    }
    let stem = Path::new(fname)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "track".into());
    let ext = Path::new(fname)
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "mp3".into());
    for i in 2..1000 {
        let cand = dir.join(format!("{} ({}).{}", stem, i, ext));
        if !cand.exists() {
            return cand;
        }
    }
    let _ = &mut p;
    dir.join(format!("{}-{}.{}", stem, std::process::id(), ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_illegal_chars() {
        assert_eq!(sanitize_filename("a/b:c*d?e\"f<g>h|i"), "a_b_c_d_e_f_g_h_i");
        assert_eq!(sanitize_filename("  多  空格  "), "多 空格");
    }

    #[test]
    fn builds_filename() {
        let t = Track {
            title: "稻香".into(),
            artist: "周杰伦".into(),
            ..Default::default()
        };
        assert_eq!(build_filename(&t, "mp3"), "周杰伦 - 稻香.mp3");
    }

    #[test]
    fn empty_title_fallback() {
        let t = Track::default();
        assert_eq!(sanitize_filename(""), "unknown");
        assert!(build_filename(&t, "mp3").ends_with(".mp3"));
    }

    #[test]
    fn unique_path_avoids_collision() {
        let dir = std::env::temp_dir().join("tinymusic_dl_test");
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a - b.mp3"), b"x").unwrap();
        let p = unique_path(&dir, "a - b.mp3");
        assert_eq!(p.file_name().unwrap(), "a - b (2).mp3");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
