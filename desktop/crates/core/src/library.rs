//! 网盘库（S3）的纯业务逻辑：歌单推导、曲目行、封面/歌词兄弟文件、收藏（「喜欢」目录）。
//!
//! 桌面（crates/app）与 iOS（crates/ffi）共用这一份；平台壳只负责把
//! S3 配置、Storage 和当前页面串起来。这里不依赖任何 UI / Tauri / Lynx。

use std::collections::HashMap;

use serde::Serialize;

use crate::download::build_filename;
use crate::model::{AudioFormat, Availability, Codec, LyricLine, Quality, Track};
use crate::s3::S3;
use crate::storage::Storage;

/// 根目录散歌在网盘页里的伪歌单名（不是真实目录，保留名不可用来建歌单）
pub const ROOT_PLAYLIST_LABEL: &str = "未分类";

/// 网盘里的「喜欢」歌单目录名
pub const FAVORITES_PLAYLIST: &str = "喜欢";

/// 封面 / 歌词直链的默认有效期（秒）
const SIGNED_EXPIRES: u64 = 6 * 3600;

#[derive(Debug, Clone, Serialize)]
pub struct S3PlaylistInfo {
    pub name: String,
    pub count: u32,
    /// 第一首歌的封面直链（决定卡片长什么样，可以为空）
    #[serde(default)]
    pub cover: Option<String>,
    /// true 表示这是「未分类」伪歌单（前缀根目录下的散歌）
    #[serde(default)]
    pub root: bool,
}

/// 网盘歌单里的一个音频文件（track.id 就是对象 key）
#[derive(Debug, Clone, Serialize)]
pub struct S3TrackRow {
    pub key: String,
    pub track: Track,
    /// 这首歌缺的附属文件：lyrics / cover（音频本身在，才可能出现在列表里）
    #[serde(default)]
    pub missing: Vec<String>,
}

/// 去掉配置的前缀，得到 歌单名/文件名 相对路径；不在前缀下的 key 返回 None
pub fn key_under_prefix<'a>(key: &'a str, prefix: &str) -> Option<&'a str> {
    let p = prefix.trim_matches('/');
    if p.is_empty() {
        return Some(key.trim_start_matches('/'));
    }
    key.strip_prefix(p)?.strip_prefix('/')
}

/// 前缀根目录下的散歌（下载时选了「未分类」）
pub fn is_root_key(key: &str, prefix: &str) -> bool {
    key_under_prefix(key, prefix)
        .map(|rest| !rest.is_empty() && !rest.contains('/'))
        .unwrap_or(false)
}

/// 从桶里的对象清单反推歌单（prefix 下的一级目录）。
pub fn playlists_from_keys(keys: &[String], prefix: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for key in keys {
        let Some(rest) = key_under_prefix(key, prefix) else {
            continue;
        };
        let Some((first, _)) = rest.split_once('/') else {
            continue;
        };
        let name = first.trim();
        if name.is_empty() {
            continue;
        }
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names.sort();
    names
}

/// 一个目录算不算歌单：里面有音频，或者有我们写的空歌单标记。
pub fn is_playlist_folder(name: &str, entries: &[String]) -> bool {
    if name.starts_with('.') {
        return false;
    }
    entries.iter().any(|key| {
        is_audio_key(key)
            || key
                .rsplit('/')
                .next()
                .map(|f| f == ".keep")
                .unwrap_or(false)
    })
}

/// 列歌单的结果。
#[derive(Debug, Clone)]
pub struct S3PlaylistListing {
    /// 歌单列表；listed == false 时只有本地记录，count 全是 0
    pub playlists: Vec<S3PlaylistInfo>,
    /// 这次到底列没列到桶：false = 离线 / 连不上 / 鉴权失败
    pub listed: bool,
}

/// 本地记录的歌单 + 网盘上的目录，并统计曲目数。
///
/// 列不到桶（离线 / 配置错误）时只返回本地记录，不把列表清空。
/// 想区分「列到了但确实为空」和「压根没列到」，用 [`list_s3_playlists_report`]。
pub async fn list_s3_playlists(
    client: &S3,
    storage: &Storage,
) -> Result<Vec<S3PlaylistInfo>, String> {
    Ok(list_s3_playlists_report(client, storage).await?.playlists)
}

/// 同 [`list_s3_playlists`]，但把「这次到底列没列到桶」一并给出来。
///
/// 壳层靠它区分「网盘上真没有歌单」和「离线时只拿得到本地记录」：
/// 后者如果当成加载成功，界面就会显示一堆 0 首的空歌单，而不是退回离线缓存。
pub async fn list_s3_playlists_report(
    client: &S3,
    storage: &Storage,
) -> Result<S3PlaylistListing, String> {
    let mut names = storage.list_s3_playlists().map_err(|e| e.to_string())?;

    let mut folders: HashMap<String, Vec<String>> = HashMap::new();
    let mut root_keys: Vec<String> = Vec::new();
    let prefix = client.normalized_prefix();
    let scope = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix)
    };
    let mut listed = false;
    if let Ok(keys) = client.list_keys_all(&scope).await {
        listed = true;
        for key in &keys {
            if is_root_key(key, &prefix) {
                root_keys.push(key.clone());
            }
            let Some(rest) = key_under_prefix(key, &prefix) else {
                continue;
            };
            let Some((folder, _)) = rest.split_once('/') else {
                continue;
            };
            folders
                .entry(folder.to_string())
                .or_default()
                .push(key.clone());
        }
        for name in playlists_from_keys(&keys, &prefix) {
            let Some(entries) = folders.get(&name) else {
                continue;
            };
            // 「喜欢」只是兜底存储，不是歌单，不列进来
            if name == FAVORITES_PLAYLIST {
                continue;
            }
            if !is_playlist_folder(&name, entries) {
                continue;
            }
            if !names.iter().any(|n| n == &name) {
                let _ = storage.save_s3_playlist(&name);
                names.push(name);
            }
        }
    }

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut count = 0u32;
        let mut cover = None;
        if let Some(keys) = folders.get(&name) {
            count = keys.iter().filter(|k| is_audio_key(k)).count() as u32;
            if let Some(first) = keys.iter().find(|k| is_audio_key(k)) {
                let lower: Vec<(String, String)> =
                    keys.iter().map(|k| (k.to_lowercase(), k.clone())).collect();
                let stem = first
                    .rsplit_once('.')
                    .map(|(a, _)| a)
                    .unwrap_or(first.as_str());
                if let Some(img) = find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"]) {
                    cover = client.presign_get(&img, SIGNED_EXPIRES).ok();
                }
            }
        }
        out.push(S3PlaylistInfo {
            name,
            count,
            cover,
            root: false,
        });
    }

    // 根目录散歌：有歌才显示「未分类」，空的时候不占位置
    if root_keys.iter().any(|k| is_audio_key(k)) {
        let count = root_keys.iter().filter(|k| is_audio_key(k)).count() as u32;
        let mut cover = None;
        if let Some(first) = root_keys.iter().find(|k| is_audio_key(k)) {
            let lower: Vec<(String, String)> = root_keys
                .iter()
                .map(|k| (k.to_lowercase(), k.clone()))
                .collect();
            let stem = first
                .rsplit_once('.')
                .map(|(a, _)| a)
                .unwrap_or(first.as_str());
            if let Some(img) = find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"]) {
                cover = client.presign_get(&img, SIGNED_EXPIRES).ok();
            }
        }
        let label = if out.iter().any(|p| p.name == ROOT_PLAYLIST_LABEL) {
            format!("{}（根目录）", ROOT_PLAYLIST_LABEL)
        } else {
            ROOT_PLAYLIST_LABEL.to_string()
        };
        out.push(S3PlaylistInfo {
            name: label,
            count,
            cover,
            root: true,
        });
    }
    Ok(S3PlaylistListing {
        playlists: out,
        listed,
    })
}

pub fn parse_track_filename(stem: &str) -> (String, String) {
    let name = stem.rsplit('/').next().unwrap_or(stem).trim();
    match name.split_once(" - ") {
        Some((artist, title)) => (artist.trim().to_string(), title.trim().to_string()),
        None => (String::new(), name.to_string()),
    }
}

/// 在对象列表里找同名不同后缀的兄弟文件（歌词/封面）
pub fn find_sibling(keys: &[(String, String)], stem: &str, exts: &[&str]) -> Option<String> {
    for ext in exts {
        let target = format!("{}.{}", stem, ext).to_lowercase();
        if let Some((_, key)) = keys
            .iter()
            .find(|(lower, _)| lower.to_lowercase() == target)
        {
            return Some(key.clone());
        }
    }
    None
}

/// 给网盘音频对象签一个封面直链：同目录同名 jpg/jpeg/png/webp。
pub async fn signed_cover_for(client: &S3, audio_key: &str) -> Option<String> {
    let (folder, _) = audio_key.rsplit_once('/')?;
    let keys = client.list_keys(&format!("{}/", folder)).await.ok()?;
    let lower: Vec<(String, String)> = keys.iter().map(|k| (k.to_lowercase(), k.clone())).collect();
    let stem = audio_key
        .rsplit_once('.')
        .map(|(a, _)| a)
        .unwrap_or(audio_key);
    find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"])
        .and_then(|k| client.presign_get(&k, SIGNED_EXPIRES).ok())
}

/// 批量刷新 s3 曲目的封面直链；同一个目录只列一次。
pub async fn refresh_s3_covers(client: &S3, tracks: &mut [Track]) {
    let mut cache: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for t in tracks.iter_mut().filter(|t| t.provider == "s3") {
        let key = t.id.clone();
        let Some((folder, _)) = key.rsplit_once('/') else {
            t.cover = None;
            continue;
        };
        let entries = match cache.get(folder).cloned() {
            Some(e) => e,
            None => {
                let entries: Vec<(String, String)> = client
                    .list_keys(&format!("{}/", folder))
                    .await
                    .unwrap_or_default()
                    .iter()
                    .map(|k| (k.to_lowercase(), k.clone()))
                    .collect();
                cache.insert(folder.to_string(), entries.clone());
                entries
            }
        };
        let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key.as_str());
        t.cover = find_sibling(&entries, stem, &["jpg", "jpeg", "png", "webp"])
            .and_then(|k| client.presign_get(&k, SIGNED_EXPIRES).ok());
    }
}

/// 把一批对象 key 变成曲目行（同一个目录里的音频 + 同名歌词/封面）
pub fn build_track_rows(client: &S3, keys: &[String]) -> Vec<S3TrackRow> {
    let lower: Vec<(String, String)> = keys.iter().map(|k| (k.to_lowercase(), k.clone())).collect();

    let mut rows = Vec::new();
    for key in keys.iter().filter(|k| is_audio_key(k)) {
        let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key.as_str());
        let (artist, title) = parse_track_filename(stem);
        let cover_key = find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"]);
        let cover = cover_key
            .as_ref()
            .and_then(|k| client.presign_get(k, SIGNED_EXPIRES).ok());

        let mut missing = Vec::new();
        if cover_key.is_none() {
            missing.push("cover".to_string());
        }
        let lrc_name = format!("{}.lrc", stem).to_lowercase();
        if !lower.iter().any(|(k, _)| *k == lrc_name) {
            missing.push("lyrics".to_string());
        }

        rows.push(S3TrackRow {
            key: key.clone(),
            missing,
            track: Track {
                provider: "s3".into(),
                id: key.clone(),
                title,
                artist,
                cover,
                availability: Availability::Available,
                available_qualities: if key.to_ascii_lowercase().ends_with(".flac") {
                    vec![Quality::Lossless]
                } else {
                    Vec::new()
                },
                ..Track::default()
            },
        });
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    rows
}

pub async fn list_s3_track_rows(client: &S3, name: &str) -> Result<Vec<S3TrackRow>, String> {
    let folder = playlist_prefix(&client.normalized_prefix(), name);
    let keys = client
        .list_keys_all(&folder)
        .await
        .map_err(|e| e.to_string())?;
    Ok(build_track_rows(client, &keys))
}

/// 前缀根目录的散歌（「未分类」）
pub async fn list_root_track_rows(client: &S3) -> Result<Vec<S3TrackRow>, String> {
    let prefix = client.normalized_prefix();
    let scope = if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix)
    };
    let keys = client
        .list_keys_all(&scope)
        .await
        .map_err(|e| e.to_string())?;
    let root: Vec<String> = keys
        .into_iter()
        .filter(|k| is_root_key(k, &prefix))
        .collect();
    Ok(build_track_rows(client, &root))
}

pub async fn s3_playlist_tracks(
    client: &S3,
    name: &str,
    root: bool,
) -> Result<Vec<S3TrackRow>, String> {
    if root {
        list_root_track_rows(client).await
    } else {
        list_s3_track_rows(client, name).await
    }
}

/// 网盘文件按后缀推断显示格式；不解码、不探测，只用于 UI 文案。
pub fn s3_audio_format(key: &str) -> (Quality, AudioFormat) {
    let lower = key.to_ascii_lowercase();
    let (codec, lossless, quality) = if lower.ends_with(".flac") {
        (Codec::Flac, true, Quality::Lossless)
    } else if lower.ends_with(".mp3") {
        (Codec::Mp3, false, Quality::Standard)
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") || lower.ends_with(".aac") {
        (Codec::Aac, false, Quality::Standard)
    } else if lower.ends_with(".ogg") {
        (Codec::Ogg, false, Quality::Standard)
    } else if lower.ends_with(".wav") {
        (Codec::Wav, true, Quality::Lossless)
    } else {
        return (Quality::Standard, AudioFormat::default());
    };
    (
        quality,
        AudioFormat {
            codec,
            bitrate_kbps: None,
            sample_rate_hz: None,
            bits_per_sample: None,
            channels: None,
            lossless,
            verified: false,
        },
    )
}

pub fn is_audio_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [".mp3", ".flac", ".m4a", ".aac", ".ogg", ".wav"]
        .iter()
        .any(|e| lower.ends_with(e))
}

pub fn audio_content_type(key: &str) -> &'static str {
    let lower = key.to_ascii_lowercase();
    if lower.ends_with(".flac") {
        "audio/flac"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else {
        "audio/mpeg"
    }
}

pub fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

/// 校验用户输入的歌单名：它最终会成为 S3 前缀目录。
pub fn sanitize_playlist_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("歌单名不能为空".into());
    }
    if name == "." || name == ".." {
        return Err("歌单名不能是 . 或 ..".into());
    }
    if name.starts_with('.') {
        return Err("歌单名不能以点开头".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("歌单名不能包含 / 或 \\".into());
    }
    if name == FAVORITES_PLAYLIST {
        return Err("「喜欢」是保留名".into());
    }
    if name == ROOT_PLAYLIST_LABEL {
        return Err("「未分类」是根目录专用名".into());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("歌单名包含不可见字符".into());
    }
    Ok(name.to_string())
}

/// 新建歌单：本地记录 + 写一个 .keep 标记，空歌单也能被其他设备发现。
pub async fn create_s3_playlist(
    client: &S3,
    storage: &Storage,
    name: &str,
) -> Result<String, String> {
    let name = sanitize_playlist_name(name)?;
    let existing = storage.list_s3_playlists().map_err(|e| e.to_string())?;
    if existing.iter().any(|n| n == &name) {
        return Err(format!("已经有叫「{name}」的歌单了"));
    }
    storage.save_s3_playlist(&name).map_err(|e| e.to_string())?;
    let key = format!(
        "{}.keep",
        playlist_prefix(&client.normalized_prefix(), &name)
    );
    let _ = client
        .put_object(&key, Vec::new(), "application/octet-stream")
        .await;
    Ok(name)
}

/// 删除歌单：连同云端对象一起删。
pub async fn purge_s3_playlist(client: &S3, storage: &Storage, name: &str) -> Result<(), String> {
    let name = sanitize_playlist_name(name)?;
    let folder = playlist_prefix(&client.normalized_prefix(), &name);
    let keys = client.list_keys(&folder).await.map_err(|e| e.to_string())?;
    for key in &keys {
        client
            .delete_object(key)
            .await
            .map_err(|e| format!("删除 {key} 失败：{e}"))?;
    }
    // 删掉的歌如果还是「喜欢」，清单里跟着清（同歌别处还有一份就改指向）
    let _ = crate::favorites::on_objects_removed(client, storage, &keys).await;
    storage.remove_s3_playlist(&name).map_err(|e| e.to_string())
}

/// 重命名歌单：把旧目录里的对象逐个搬到新目录。
pub async fn rename_s3_playlist(
    client: &S3,
    storage: &Storage,
    old: &str,
    new: &str,
) -> Result<String, String> {
    let old = sanitize_playlist_name(old)?;
    let new = sanitize_playlist_name(new)?;
    if old == new {
        return Ok(new);
    }
    let existing = storage.list_s3_playlists().map_err(|e| e.to_string())?;
    if existing.iter().any(|n| n == &new) {
        return Err(format!("已经有叫「{new}」的歌单了"));
    }
    let prefix = client.normalized_prefix();
    let old_folder = playlist_prefix(&prefix, &old);
    let new_folder = playlist_prefix(&prefix, &new);
    let keys = client
        .list_keys(&old_folder)
        .await
        .map_err(|e| e.to_string())?;
    for key in keys {
        let suffix = key.strip_prefix(&old_folder).unwrap_or(&key);
        client
            .move_object(&key, &format!("{new_folder}{suffix}"))
            .await
            .map_err(|e| format!("移动 {key} 失败：{e}"))?;
    }
    let _ = crate::favorites::rekey(client, storage, &old_folder, &new_folder).await;
    storage
        .remove_s3_playlist(&old)
        .map_err(|e| e.to_string())?;
    storage.save_s3_playlist(&new).map_err(|e| e.to_string())?;
    Ok(new)
}

/// 删除单曲：音频 + 同名歌词/封面，并从「喜欢」里清掉。
pub async fn delete_s3_track(client: &S3, storage: &Storage, key: &str) -> Result<(), String> {
    if key.trim().is_empty() {
        return Err("曲目 key 为空".into());
    }
    client.delete_object(key).await.map_err(|e| e.to_string())?;
    let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key);
    for ext in ["lrc", "jpg", "jpeg", "png", "webp"] {
        let _ = client.delete_object(&format!("{stem}.{ext}")).await;
    }
    let _ = crate::favorites::on_objects_removed(client, storage, &[key.to_string()]).await;
    Ok(())
}

/// 把单曲（含歌词/封面）移动到另一个歌单目录。
pub async fn move_s3_track(
    client: &S3,
    storage: &Storage,
    key: &str,
    target: &str,
) -> Result<(), String> {
    let target = sanitize_playlist_name(target)?;
    if key.trim().is_empty() {
        return Err("曲目 key 为空".into());
    }
    let prefix = client.normalized_prefix();
    let Some(rest) = key_under_prefix(key, &prefix) else {
        return Err("曲目不在当前 S3 前缀下".into());
    };
    let (current, filename) = match rest.split_once('/') {
        Some((folder, file)) => (folder, file),
        None => ("", rest),
    };
    if current == target {
        return Err("已经在目标歌单里".into());
    }
    let old_folder = playlist_prefix(&prefix, current);
    let new_folder = playlist_prefix(&prefix, &target);
    storage
        .save_s3_playlist(&target)
        .map_err(|e| e.to_string())?;
    let _ = client
        .put_object(
            &format!("{new_folder}.keep"),
            Vec::new(),
            "application/octet-stream",
        )
        .await;
    client
        .move_object(key, &format!("{new_folder}{filename}"))
        .await
        .map_err(|e| format!("移动 {key} 失败：{e}"))?;
    let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key);
    for ext in ["lrc", "jpg", "jpeg", "png", "webp"] {
        let from = format!("{stem}.{ext}");
        if !from.starts_with(&old_folder) {
            continue;
        }
        let sibling = from.strip_prefix(&old_folder).unwrap_or(&from);
        let _ = client
            .move_object(&from, &format!("{new_folder}{sibling}"))
            .await;
    }
    // 收藏清单里指向这首的条目跟着改 key：移动不是复制，喜欢不变。
    let old_key = key.to_string();
    let new_key = format!("{new_folder}{filename}");
    let _ = crate::favorites::rekey(client, storage, &old_key, &new_key).await;
    Ok(())
}

/// 歌单在网盘里就是一个前缀目录
pub fn playlist_prefix(prefix: &str, playlist: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    let pl = playlist.trim().trim_matches('/');
    match (p.is_empty(), pl.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{}/", p),
        (true, false) => format!("{}/", pl),
        (false, false) => format!("{}/{}/", p, pl),
    }
}

pub fn object_key(prefix: &str, playlist: &str, track: &Track, ext: &str) -> String {
    format!(
        "{}{}",
        playlist_prefix(prefix, playlist),
        build_filename(track, ext)
    )
}

pub fn replace_ext(key: &str, ext: &str) -> String {
    match key.rfind('.') {
        Some(i) if !key[i + 1..].contains('/') => format!("{}.{}", &key[..i], ext),
        _ => format!("{}.{}", key, ext),
    }
}

/// LRC 文本 → 歌词行（同一句可以挂多个时间戳）
pub fn parse_lrc(text: &str) -> Vec<LyricLine> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut stamps = Vec::new();
        while let Some(tail) = rest.strip_prefix('[') {
            let Some(end) = tail.find(']') else { break };
            let tag = &tail[..end];
            match tag.split_once(':').and_then(|(m, s)| {
                let m: f64 = m.trim().parse().ok()?;
                let s: f64 = s.trim().parse().ok()?;
                Some(m * 60_000.0 + s * 1000.0)
            }) {
                Some(ms) => {
                    stamps.push(ms.max(0.0) as u32);
                    rest = &tail[end + 1..];
                }
                None => break,
            }
        }
        let line = rest.trim();
        for ms in stamps {
            out.push(LyricLine {
                time_ms: ms,
                text: line.to_string(),
            });
        }
    }
    out.sort_by_key(|l| l.time_ms);
    out
}

/// 「喜欢」目录的完整前缀（带上网盘前缀）
pub fn favorites_folder(prefix: &str) -> String {
    playlist_prefix(prefix, FAVORITES_PLAYLIST)
}

/// key 是不是「喜欢」目录里的对象
pub fn is_favorite_key(key: &str, prefix: &str) -> bool {
    key.starts_with(&favorites_folder(prefix))
}


/// 同一首歌的判定键：歌名 + 歌手，去掉大小写和标点
pub fn favorite_key(title: &str, artist: &str) -> String {
    fn normalize(value: &str) -> String {
        value
            .chars()
            .flat_map(|c| c.to_lowercase())
            .filter(|c| c.is_alphanumeric())
            .collect()
    }
    format!("{}|{}", normalize(title), normalize(artist))
}

pub fn favorite_dedupe_key(t: &Track) -> String {
    favorite_key(&t.title, &t.artist)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_under_prefix_handles_empty_and_nested() {
        assert_eq!(
            key_under_prefix("TinyMusic/A/a.mp3", "TinyMusic"),
            Some("A/a.mp3")
        );
        assert_eq!(key_under_prefix("A/a.mp3", ""), Some("A/a.mp3"));
        assert_eq!(key_under_prefix("Other/A/a.mp3", "TinyMusic"), None);
        assert!(is_root_key("TinyMusic/root.mp3", "TinyMusic"));
        assert!(!is_root_key("TinyMusic/A/a.mp3", "TinyMusic"));
    }

    #[test]
    fn playlists_are_sorted_and_ignore_root_files() {
        let keys = vec![
            "TinyMusic/B/b.mp3".to_string(),
            "TinyMusic/A/a.mp3".to_string(),
            "TinyMusic/root.mp3".to_string(),
            "Other/C/c.mp3".to_string(),
        ];
        assert_eq!(playlists_from_keys(&keys, "TinyMusic"), vec!["A", "B"]);
    }

    #[test]
    fn only_audio_or_keep_marker_counts_as_playlist() {
        assert!(is_playlist_folder("A", &["A/a.mp3".to_string()]));
        assert!(is_playlist_folder("A", &["A/.keep".to_string()]));
        assert!(!is_playlist_folder("A", &["A/cover.jpg".to_string()]));
        assert!(!is_playlist_folder(
            ".hidden",
            &[".hidden/a.mp3".to_string()]
        ));
    }

    #[test]
    fn track_filename_prefers_artist_dash_title() {
        assert_eq!(
            parse_track_filename("TinyMusic/A/周杰伦 - 东风破"),
            ("周杰伦".into(), "东风破".into())
        );
        assert_eq!(
            parse_track_filename("A/纯音乐"),
            ("".into(), "纯音乐".into())
        );
    }

    #[test]
    fn sibling_lookup_is_case_insensitive() {
        let keys = vec![
            ("a/b.JPG".to_string(), "A/B.JPG".to_string()),
            ("a/b.lrc".to_string(), "A/B.lrc".to_string()),
        ];
        assert_eq!(
            find_sibling(&keys, "A/B", &["jpg"]),
            Some("A/B.JPG".to_string())
        );
    }

    #[test]
    fn lrc_parses_multiple_stamps_and_sorts() {
        let lines = parse_lrc("[00:02.00]two\n[00:01.00][00:03.00]one");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].time_ms, 1000);
        assert_eq!(lines[1].time_ms, 2000);
        assert_eq!(lines[2].time_ms, 3000);
    }

    #[test]
    fn audio_key_accepts_aac_and_rejects_images() {
        assert!(is_audio_key("a.mp3"));
        assert!(is_audio_key("a.FLAC"));
        assert!(is_audio_key("a.aac"));
        assert!(!is_audio_key("a.jpg"));
    }

    #[test]
    fn s3_format_maps_known_extensions() {
        assert_eq!(s3_audio_format("a.flac").0, Quality::Lossless);
        assert_eq!(s3_audio_format("a.m4a").1.codec, Codec::Aac);
        assert_eq!(s3_audio_format("a.unknown").1.codec, Codec::Unknown);
    }

    #[test]
    fn prefixes_compose() {
        assert_eq!(playlist_prefix("TinyMusic", "A"), "TinyMusic/A/");
        assert_eq!(playlist_prefix("", "A"), "A/");
        assert_eq!(playlist_prefix("TinyMusic", ""), "TinyMusic/");
    }

    #[test]
    fn favorites_live_under_liked_folder() {
        assert_eq!(favorites_folder("TinyMusic"), "TinyMusic/喜欢/");
        assert_eq!(favorites_folder(""), "喜欢/");
        assert!(is_favorite_key("TinyMusic/喜欢/a.mp3", "TinyMusic"));
        assert!(!is_favorite_key("TinyMusic/A/a.mp3", "TinyMusic"));
        // 收藏本体是清单文件，「喜欢」目录只是兜底存储
        assert_eq!(
            crate::favorites::manifest_key("TinyMusic"),
            "TinyMusic/favorites.json"
        );
        assert_eq!(crate::favorites::manifest_key(""), "favorites.json");
    }

    #[test]
    fn favorite_key_ignores_case_and_punctuation() {
        assert_eq!(favorite_key("5:20AM", "Soldout"), favorite_key("5 20am", "SOLDOUT"));
        assert_ne!(favorite_key("A", "X"), favorite_key("B", "X"));
    }

    #[test]
    fn playlist_names_are_validated() {
        assert_eq!(sanitize_playlist_name("  夜航  ").unwrap(), "夜航");
        assert!(sanitize_playlist_name("").is_err());
        assert!(sanitize_playlist_name("a/b").is_err());
        assert!(sanitize_playlist_name("..").is_err());
        assert!(sanitize_playlist_name(FAVORITES_PLAYLIST).is_err());
        assert!(sanitize_playlist_name(ROOT_PLAYLIST_LABEL).is_err());
    }

    /// 离线刷新歌单：core 只拿得到本地记录（count 全 0），必须同时报告「没列到桶」，
    /// 壳层才能退回离线缓存，而不是显示一堆 0 首的空歌单。
    #[tokio::test]
    async fn offline_listing_reports_not_listed() {
        use crate::http::HttpClient;
        use crate::s3::{S3Config, S3};
        use crate::storage::Storage;

        let dir = std::env::temp_dir().join(format!("tm-core-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = Storage::open(dir.join("t.db")).unwrap();
        storage.save_s3_playlist("通勤").unwrap();

        // 127.0.0.1:1 会被立刻拒绝：等价于「没网 / 连不上桶」
        let client = S3::new(
            S3Config {
                endpoint: "http://127.0.0.1:1".into(),
                bucket: "music".into(),
                access_key: "ak".into(),
                secret_key: "sk".into(),
                prefix: "TinyMusic".into(),
                path_style: true,
                ..Default::default()
            },
            HttpClient::new(0, 1).client().clone(),
        );

        let listing = list_s3_playlists_report(&client, &storage).await.unwrap();
        assert!(!listing.listed, "连不上桶时必须报告 listed = false");
        assert_eq!(listing.playlists.len(), 1);
        assert_eq!(listing.playlists[0].name, "通勤");
        assert_eq!(listing.playlists[0].count, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
