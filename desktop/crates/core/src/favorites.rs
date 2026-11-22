//! 收藏（「喜欢」）：网盘 favorites.json 是唯一真源，「喜欢」目录只兜底存
//! 网盘其它地方都没有的歌。
//!
//! 桌面（crates/app）与 iOS（crates/ffi）共用这一份：
//! - 判断桶里有没有这首歌、写清单、条件删对象都在这里；
//! - 平台歌曲的取流/上传需要 provider，由 app 负责，这里只给目标 key。

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::download::build_filename;
use crate::library::{
    favorite_key, favorites_folder, find_sibling, is_audio_key, is_favorite_key,
    parse_track_filename,
};
use crate::model::{Availability, Quality, Track};
use crate::s3::S3;
use crate::storage::Storage;

/// 清单文件名（相对配置的前缀目录）
pub const MANIFEST_FILE: &str = "favorites.json";
/// 清单格式版本：只认这个版本，别的版本不覆盖
pub const MANIFEST_VERSION: u32 = 2;

const MIGRATED_META: &str = "s3_favorites_json_v2";
const AUDIO_EXTS: [&str; 6] = ["mp3", "m4a", "aac", "flac", "wav", "ogg"];
const SIBLING_EXTS: [&str; 5] = ["lrc", "jpg", "jpeg", "png", "webp"];
const SIGNED_EXPIRES: u64 = 6 * 3600;

/// 清单里的一条喜欢：key 指向桶里真实存在的音频对象（任意歌单目录或 喜欢/）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteEntry {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    pub key: String,
    #[serde(default)]
    pub added_at: i64,
}

impl FavoriteEntry {
    pub fn dedupe_key(&self) -> String {
        favorite_key(&self.title, &self.artist)
    }
}

/// 网盘里的收藏清单
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoritesManifest {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub items: Vec<FavoriteEntry>,
}

impl Default for FavoritesManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            updated_at: 0,
            items: Vec::new(),
        }
    }
}

/// 点喜欢的结果：已经落库，或者需要 app 先把音频传到 喜欢/ 再 commit
#[derive(Debug, Clone)]
pub enum FavoriteAdd {
    Added { key: String },
    NeedsAudio { hint_key: String },
}

/// 去重维护的一条：duplicate 是「喜欢」目录里那份，canonical 是保留的那份
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateCopy {
    pub canonical: String,
    pub duplicate: String,
    pub bytes: u64,
}

pub fn manifest_key(prefix: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    if p.is_empty() {
        MANIFEST_FILE.to_string()
    } else {
        format!("{p}/{MANIFEST_FILE}")
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------- 清单读写 ----------

pub async fn load_manifest(client: &S3) -> Result<Option<FavoritesManifest>, String> {
    let key = manifest_key(&client.normalized_prefix());
    let bytes = client.get_object(&key).await.map_err(|e| e.to_string())?;
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes);
    let manifest: FavoritesManifest = serde_json::from_str(&text)
        .map_err(|e| format!("喜欢清单格式不对（{key}）：{e}"))?;
    if manifest.version != MANIFEST_VERSION {
        return Err(format!(
            "喜欢清单版本 {} 不认识（本版本只认 {}），没有覆盖它",
            manifest.version, MANIFEST_VERSION
        ));
    }
    Ok(Some(manifest))
}

/// 写盘前统一整理：版本、时间戳、同一首歌只留一条、按加入时间倒序
fn normalized(mut manifest: FavoritesManifest, now: i64) -> FavoritesManifest {
    manifest.version = MANIFEST_VERSION;
    manifest.updated_at = now;
    for entry in &mut manifest.items {
        if entry.added_at == 0 {
            entry.added_at = now;
        }
    }
    // 同一首歌只留最早加入的那条：先按时间正序去重，再按时间倒序展示
    manifest
        .items
        .sort_by(|a, b| a.added_at.cmp(&b.added_at).then_with(|| a.key.cmp(&b.key)));
    let mut seen = HashSet::new();
    manifest.items.retain(|e| seen.insert(e.dedupe_key()));
    manifest
        .items
        .sort_by(|a, b| b.added_at.cmp(&a.added_at).then_with(|| a.key.cmp(&b.key)));
    manifest
}

pub async fn save_manifest(client: &S3, manifest: &FavoritesManifest) -> Result<(), String> {
    let out = normalized(manifest.clone(), now_secs());
    let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    let key = manifest_key(&client.normalized_prefix());
    client
        .put_object(&key, text.into_bytes(), "application/json; charset=utf-8")
        .await
        .map_err(|e| e.to_string())
}

// ---------- 桶索引 ----------

/// 一次 list_keys_all 建出来的桶索引：
/// 判断 key 在不在、按「同一首歌」找候选、按目录找同名歌词/封面。
#[derive(Debug, Default)]
pub struct BucketIndex {
    keys: HashSet<String>,
    folders: HashMap<String, Vec<String>>,
    by_dedupe: HashMap<String, Vec<String>>,
    by_stem: HashMap<String, Vec<String>>,
    by_title: HashMap<String, Vec<String>>,
    favorites: String,
}

impl BucketIndex {
    pub fn contains(&self, key: &str) -> bool {
        self.keys.contains(key)
    }

    fn folder_entries(&self, key: &str) -> &[String] {
        match key.rsplit_once('/') {
            Some((folder, _)) => self.folders.get(folder).map(|v| v.as_slice()).unwrap_or(&[]),
            None => self.folders.get("").map(|v| v.as_slice()).unwrap_or(&[]),
        }
    }

    /// 同一首歌的最优副本：优先不在「喜欢」目录里，再按路径短、字典序
    fn pick(&self, candidates: &[String]) -> Option<String> {
        candidates
            .iter()
            .min_by(|a, b| {
                let rank = |k: &String| usize::from(k.starts_with(&self.favorites));
                rank(a)
                    .cmp(&rank(b))
                    .then_with(|| a.len().cmp(&b.len()))
                    .then_with(|| a.cmp(b))
            })
            .cloned()
    }

    /// 按曲目找桶里已有的副本：先按歌名/歌手，再按我们上传时会用的文件名
    /// （标题里带 " - " 时解析会分错，精确文件名能兜住）
    pub fn lookup(&self, track: &Track) -> Option<String> {
        if let Some(found) = self.canonical_for(&track.title, &track.artist) {
            return Some(found);
        }
        for ext in AUDIO_EXTS {
            let stem = build_filename(track, ext).to_lowercase();
            if let Some(list) = self.by_stem.get(&stem) {
                if let Some(found) = self.pick(list) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// 按歌名 + 歌手找桶里已有的副本
    pub fn canonical_for(&self, title: &str, artist: &str) -> Option<String> {
        let dedupe = favorite_key(title, artist);
        if let Some(list) = self.by_dedupe.get(&dedupe) {
            if let Some(found) = self.pick(list) {
                return Some(found);
            }
        }
        let title_key = normalize(title);
        if title_key.is_empty() {
            return None;
        }
        self.by_title
            .get(&title_key)
            .and_then(|list| self.pick(list))
    }

    /// 给定对象 key，找它这一首歌的最优副本（key 本身在桶里时至少返回它自己）
    pub fn canonical_near(&self, key: &str) -> Option<String> {
        let (artist, title) = parse_track_filename(stem_of(key));
        if let Some(found) = self.canonical_for(&title, &artist) {
            return Some(found);
        }
        if self.contains(key) {
            return Some(key.to_string());
        }
        if let Some(list) = self.by_stem.get(&stem_lower(key)) {
            return self.pick(list);
        }
        None
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| c.to_lowercase())
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn stem_of(key: &str) -> &str {
    key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key)
}

fn stem_lower(key: &str) -> String {
    stem_of(key).to_lowercase()
}

pub async fn bucket_index(client: &S3) -> Result<BucketIndex, String> {
    let prefix = client.normalized_prefix();
    let scope = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    };
    let keys = client
        .list_keys_all(&scope)
        .await
        .map_err(|e| e.to_string())?;
    Ok(BucketIndex::from_keys(&keys, &favorites_folder(&prefix)))
}

impl BucketIndex {
    fn from_keys(keys: &[String], favorites: &str) -> Self {
        let mut index = BucketIndex {
            favorites: favorites.to_string(),
            ..Default::default()
        };
        for key in keys {
            index.keys.insert(key.clone());
            let folder = key
                .rsplit_once('/')
                .map(|(f, _)| f.to_string())
                .unwrap_or_default();
            index.folders.entry(folder).or_default().push(key.clone());
            if !is_audio_key(key) {
                continue;
            }
            let (artist, title) = parse_track_filename(stem_of(key));
            index
                .by_dedupe
                .entry(favorite_key(&title, &artist))
                .or_default()
                .push(key.clone());
            index
                .by_stem
                .entry(stem_lower(key))
                .or_default()
                .push(key.clone());
            let title_key = normalize(&title);
            if !title_key.is_empty() {
                index.by_title.entry(title_key).or_default().push(key.clone());
            }
        }
        index
    }
}

// ---------- 曲目行 ----------

fn track_for(client: &S3, index: &BucketIndex, key: &str) -> Option<Track> {
    if !index.contains(key) {
        return None;
    }
    let entries: Vec<(String, String)> = index
        .folder_entries(key)
        .iter()
        .map(|k| (k.to_lowercase(), k.clone()))
        .collect();
    let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key);
    let cover = find_sibling(&entries, stem, &["jpg", "jpeg", "png", "webp"])
        .and_then(|k| client.presign_get(&k, SIGNED_EXPIRES).ok());
    let (artist, title) = parse_track_filename(stem);
    Some(Track {
        provider: "s3".into(),
        id: key.to_string(),
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
    })
}

fn entries_to_rows(client: &S3, index: &BucketIndex, manifest: &FavoritesManifest) -> Vec<Track> {
    manifest
        .items
        .iter()
        .filter_map(|e| track_for(client, index, &e.key))
        .collect()
}

// ---------- 本地镜像 ----------

fn clear_local_by_dedupe(storage: &Storage, dedupe: &str) {
    if let Ok(rows) = storage.list_favorites() {
        for t in rows {
            if favorite_key(&t.title, &t.artist) == dedupe {
                let _ = storage.remove_favorite(&t.provider, &t.id);
            }
        }
    }
}

/// 本地缓存跟清单对齐：清单里的补进来，已经不在清单里的 s3 行删掉，
/// 但 pending=1（还没写进清单）的必须留着。
fn sync_cache(storage: &Storage, rows: &[Track]) {
    let pending: HashSet<(String, String)> = storage
        .list_pending_favorites()
        .map(|v| {
            v.into_iter()
                .map(|t| (t.provider, t.id))
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let keep: HashSet<&str> = rows.iter().map(|t| t.id.as_str()).collect();
    if let Ok(local) = storage.list_favorites() {
        for t in local {
            if t.provider != "s3" || pending.contains(&(t.provider.clone(), t.id.clone())) {
                continue;
            }
            if !keep.contains(t.id.as_str()) {
                let _ = storage.remove_favorite(&t.provider, &t.id);
            }
        }
    }
    for t in rows {
        let _ = storage.upsert_track(t);
        let _ = storage.add_favorite(t);
        let _ = storage.set_favorite_pending("s3", &t.id, false);
    }
}

// ---------- 喜欢 / 取消喜欢 ----------

/// 兜底目录里给平台歌曲预留的 key（扩展名以实际上传的为准，commit 时会传真实 key）
fn hinted_key(client: &S3, track: &Track) -> String {
    let folder = favorites_folder(&client.normalized_prefix());
    let name = build_filename(track, "x");
    let stem = name.strip_suffix(".x").unwrap_or(&name);
    format!("{folder}{stem}")
}

/// 点喜欢：桶里已有这首歌就只写清单；平台歌且桶里没有则让调用方先上传。
pub async fn add(client: &S3, storage: &Storage, track: &Track) -> Result<FavoriteAdd, String> {
    let index = bucket_index(client).await?;
    let canonical = if track.provider == "s3" {
        index
            .canonical_near(&track.id)
            .or_else(|| index.canonical_for(&track.title, &track.artist))
            .ok_or_else(|| "原文件已经不在了".to_string())?
    } else {
        match index.lookup(track) {
            Some(key) => key,
            None => {
                return Ok(FavoriteAdd::NeedsAudio {
                    hint_key: hinted_key(client, track),
                })
            }
        }
    };
    commit(client, storage, track, &canonical).await?;
    Ok(FavoriteAdd::Added { key: canonical })
}

/// 音频已经在桶里（或刚上传完）时落清单 + 本地缓存。
pub async fn commit(
    client: &S3,
    storage: &Storage,
    track: &Track,
    key: &str,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("收藏目标的 key 为空".into());
    }
    let mut manifest = load_manifest(client).await?.unwrap_or_default();
    let dedupe = favorite_key(&track.title, &track.artist);
    let now = now_secs();
    if let Some(entry) = manifest.items.iter_mut().find(|e| e.dedupe_key() == dedupe) {
        entry.key = key.to_string();
        if entry.title.is_empty() {
            entry.title = track.title.clone();
        }
        if entry.artist.is_empty() {
            entry.artist = track.artist.clone();
        }
        if entry.added_at == 0 {
            entry.added_at = now;
        }
    } else {
        manifest.items.push(FavoriteEntry {
            title: track.title.clone(),
            artist: track.artist.clone(),
            key: key.to_string(),
            added_at: now,
        });
    }
    save_manifest(client, &manifest).await?;

    clear_local_by_dedupe(storage, &dedupe);
    let mut row = track.clone();
    row.provider = "s3".into();
    row.id = key.to_string();
    row.cover = None;
    let _ = storage.add_favorite(&row);
    Ok(())
}

/// 取消喜欢：清单里删条目；只有「喜欢」目录里的兜底副本才删对象。
pub async fn remove(client: &S3, storage: &Storage, key: &str) -> Result<(), String> {
    remove_inner(client, storage, key, None).await
}

/// 从一首歌（可能是平台条目）取消喜欢：按歌名 + 歌手再认一遍
pub async fn remove_track(client: &S3, storage: &Storage, track: &Track) -> Result<(), String> {
    remove_inner(
        client,
        storage,
        &track.id,
        Some((track.title.as_str(), track.artist.as_str())),
    )
    .await
}

async fn remove_inner(
    client: &S3,
    storage: &Storage,
    key: &str,
    hint: Option<(&str, &str)>,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() && hint.is_none() {
        return Err("曲目 key 为空".into());
    }
    let prefix = client.normalized_prefix();
    let dedupe = match hint {
        Some((title, artist)) if !title.trim().is_empty() || !artist.trim().is_empty() => {
            favorite_key(title, artist)
        }
        _ => {
            let (artist, title) = parse_track_filename(key);
            favorite_key(&title, &artist)
        }
    };

    let mut removed_key: Option<String> = None;
    if let Some(mut manifest) = load_manifest(client).await? {
        let before = manifest.items.len();
        manifest.items.retain(|e| {
            let hit = (!key.is_empty() && e.key == key)
                || (!dedupe.is_empty() && e.dedupe_key() == dedupe);
            if hit && removed_key.is_none() {
                removed_key = Some(e.key.clone());
            }
            !hit
        });
        if manifest.items.len() != before {
            save_manifest(client, &manifest).await?;
        }
    }

    let target = removed_key.unwrap_or_else(|| key.to_string());
    if is_favorite_key(&target, &prefix) {
        delete_object_with_siblings(client, &target).await;
    }

    if let Ok(rows) = storage.list_favorites() {
        for t in rows {
            let hit = (!key.is_empty() && t.id == key)
                || (!dedupe.is_empty() && favorite_key(&t.title, &t.artist) == dedupe);
            if hit {
                let _ = storage.remove_favorite(&t.provider, &t.id);
            }
        }
    }
    Ok(())
}

async fn delete_object_with_siblings(client: &S3, key: &str) {
    let _ = client.delete_object(key).await;
    let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key);
    for ext in SIBLING_EXTS {
        let _ = client.delete_object(&format!("{stem}.{ext}")).await;
    }
}

// ---------- 拉取 ----------

/// 拉「喜欢」：清单是唯一真源；顺带校验对象还在不在、把本地缓存对齐。
pub async fn pull(client: &S3, storage: &Storage) -> Result<Vec<Track>, String> {
    migrate_v2(client, storage).await?;
    let mut manifest = load_manifest(client).await?.unwrap_or_default();
    let index = bucket_index(client).await?;

    let mut changed = false;
    let mut kept: Vec<FavoriteEntry> = Vec::new();
    for mut entry in manifest.items.drain(..) {
        if let Some(canonical) = index.canonical_for(&entry.title, &entry.artist) {
            if canonical != entry.key {
                entry.key = canonical;
                changed = true;
            }
        }
        if !index.contains(&entry.key) {
            changed = true;
            continue;
        }
        kept.push(entry);
    }
    manifest.items = kept;
    if changed {
        let _ = save_manifest(client, &manifest).await;
    }

    let rows = entries_to_rows(client, &index, &manifest);
    sync_cache(storage, &rows);

    let mut out = rows;
    if let Ok(pending) = storage.list_pending_favorites() {
        for t in pending {
            let dedupe = favorite_key(&t.title, &t.artist);
            if !out.iter().any(|x| favorite_key(&x.title, &x.artist) == dedupe) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

/// 本地 pending 的 s3 条目补写进清单（平台条目要上传，交给 app 处理）
pub async fn push_pending_s3(client: &S3, storage: &Storage) -> Result<usize, String> {
    let pending = storage.list_pending_favorites().map_err(|e| e.to_string())?;
    if pending.is_empty() {
        return Ok(0);
    }
    let index = bucket_index(client).await?;
    let mut done = 0;
    for track in pending {
        if track.provider != "s3" {
            continue;
        }
        let Some(key) = index
            .canonical_near(&track.id)
            .or_else(|| index.canonical_for(&track.title, &track.artist))
        else {
            let _ = storage.remove_favorite(&track.provider, &track.id);
            continue;
        };
        if commit(client, storage, &track, &key).await.is_ok() {
            done += 1;
        }
    }
    Ok(done)
}

// ---------- 联动：删除 / 移动 / 改名 ----------

/// 一批对象被删掉：清单里指向它们的条目先尝试指向同歌的另一份，找不到就剪掉。
pub async fn on_objects_removed(
    client: &S3,
    storage: &Storage,
    keys: &[String],
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    let Some(mut manifest) = load_manifest(client).await? else {
        return Ok(());
    };
    let index = bucket_index(client).await?;
    let gone: HashSet<&str> = keys.iter().map(|k| k.as_str()).collect();
    let mut changed = false;
    let mut kept = Vec::new();
    for mut entry in manifest.items.drain(..) {
        if !gone.contains(entry.key.as_str()) {
            kept.push(entry);
            continue;
        }
        changed = true;
        match index.canonical_for(&entry.title, &entry.artist) {
            Some(canonical) if !gone.contains(canonical.as_str()) => {
                entry.key = canonical;
                kept.push(entry);
            }
            _ => {}
        }
    }
    manifest.items = kept;
    if changed {
        save_manifest(client, &manifest).await?;
        let rows = entries_to_rows(client, &index, &manifest);
        sync_cache(storage, &rows);
    }
    Ok(())
}

/// 对象被移动/改名：把条目里的 key 跟着改（单曲移动传完整 key，歌单改名传目录前缀）。
pub async fn rekey(
    client: &S3,
    storage: &Storage,
    old_prefix: &str,
    new_prefix: &str,
) -> Result<usize, String> {
    let Some(mut manifest) = load_manifest(client).await? else {
        return Ok(0);
    };
    let mut moved = 0;
    for entry in &mut manifest.items {
        if entry.key == old_prefix {
            entry.key = new_prefix.to_string();
            moved += 1;
        } else if let Some(rest) = entry.key.strip_prefix(old_prefix) {
            entry.key = format!("{new_prefix}{rest}");
            moved += 1;
        }
    }
    if moved > 0 {
        save_manifest(client, &manifest).await?;
        let index = bucket_index(client).await?;
        let rows = entries_to_rows(client, &index, &manifest);
        sync_cache(storage, &rows);
    }
    Ok(moved)
}

// ---------- 迁移 ----------

/// 一次性把「目录 = 收藏」迁成 JSON 清单。不删任何文件。
pub async fn migrate_v2(client: &S3, storage: &Storage) -> Result<(), String> {
    if storage.meta_get(MIGRATED_META).is_some() {
        return Ok(());
    }
    let index = bucket_index(client).await?;

    // 已经有清单（新版本写过）：只补本地镜像
    if let Some(manifest) = load_manifest(client).await? {
        let rows = entries_to_rows(client, &index, &manifest);
        sync_cache(storage, &rows);
        let _ = storage.meta_set(MIGRATED_META, "1");
        return Ok(());
    }

    let prefix = client.normalized_prefix();
    let folder = favorites_folder(&prefix);
    let now = now_secs();
    let mut items: Vec<FavoriteEntry> = Vec::new();

    // 1) 「喜欢」目录里已经存在的副本
    let folder_trim = folder.trim_end_matches('/').to_string();
    let mut folder_keys: Vec<String> = index
        .folders
        .get(&folder_trim)
        .cloned()
        .unwrap_or_default();
    folder_keys.sort();
    for key in folder_keys.into_iter().filter(|k| is_audio_key(k)) {
        let (artist, title) = parse_track_filename(stem_of(&key));
        let canonical = index.canonical_for(&title, &artist).unwrap_or_else(|| key.clone());
        items.push(FavoriteEntry {
            title,
            artist,
            key: canonical,
            added_at: now,
        });
    }

    // 2) 本地收藏：s3 的补进清单；平台歌标 pending，交给后续同步上传
    let mut seen: HashSet<String> = items.iter().map(|e| e.dedupe_key()).collect();
    for (track, added_at) in storage.list_favorites_with_time().map_err(|e| e.to_string())? {
        if track.provider != "s3" {
            let _ = storage.set_favorite_pending(&track.provider, &track.id, true);
            continue;
        }
        let canonical = index
            .canonical_near(&track.id)
            .or_else(|| index.canonical_for(&track.title, &track.artist));
        let Some(key) = canonical else {
            continue;
        };
        let dedupe = favorite_key(&track.title, &track.artist);
        if !seen.insert(dedupe) {
            continue;
        }
        items.push(FavoriteEntry {
            title: track.title,
            artist: track.artist,
            key,
            added_at: if added_at > 0 { added_at } else { now },
        });
    }

    let manifest = FavoritesManifest {
        version: MANIFEST_VERSION,
        updated_at: now,
        items,
    };
    save_manifest(client, &manifest).await?;
    let rows = entries_to_rows(client, &index, &manifest);
    sync_cache(storage, &rows);
    let _ = storage.meta_set(MIGRATED_META, "1");
    Ok(())
}

// ---------- 去重维护（桌面端手动触发） ----------

/// 扫「喜欢」目录里跟别的歌单重复的副本
pub async fn dedupe_report(client: &S3) -> Result<Vec<DuplicateCopy>, String> {
    let index = bucket_index(client).await?;
    let folder = favorites_folder(&client.normalized_prefix());
    let mut out = Vec::new();
    for keys in index.by_dedupe.values() {
        let Some(canonical) = index.pick(keys) else {
            continue;
        };
        if canonical.starts_with(&folder) {
            continue;
        }
        for key in keys {
            if key != &canonical && key.starts_with(&folder) {
                let bytes = client.head_object_size(key).await.ok().flatten().unwrap_or(0);
                out.push(DuplicateCopy {
                    canonical: canonical.clone(),
                    duplicate: key.clone(),
                    bytes,
                });
            }
        }
    }
    out.sort_by(|a, b| a.duplicate.cmp(&b.duplicate));
    Ok(out)
}

/// 删掉「喜欢」目录里的重复副本，返回释放的字节数。只动「喜欢」目录。
pub async fn dedupe_purge(client: &S3, keys: &[String]) -> Result<u64, String> {
    let folder = favorites_folder(&client.normalized_prefix());
    // 删之前再扫一遍：确认歌单里那份还在，别把唯一一份删了
    let verified: HashSet<String> = dedupe_report(client)
        .await?
        .into_iter()
        .map(|d| d.duplicate)
        .collect();
    let mut freed = 0u64;
    for key in keys {
        if !key.starts_with(&folder) || !is_audio_key(key) || !verified.contains(key) {
            continue;
        }
        freed += client.head_object_size(key).await.ok().flatten().unwrap_or(0);
        delete_object_with_siblings(client, key).await;
    }
    Ok(freed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Track;

    fn idx(keys: &[&str]) -> BucketIndex {
        let owned: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
        BucketIndex::from_keys(&owned, "TinyMusic/喜欢/")
    }

    #[test]
    fn manifest_normalizes_and_dedupes_by_song() {
        let first = FavoriteEntry {
            title: "5:20AM".into(),
            artist: "刀酱".into(),
            key: "TinyMusic/喜欢/刀酱 - 5_20AM.flac".into(),
            added_at: 100,
        };
        let dup = FavoriteEntry {
            title: "5 20am".into(),
            artist: "刀酱".into(),
            key: "TinyMusic/A/刀酱 - 5_20AM.flac".into(),
            added_at: 200,
        };
        let other = FavoriteEntry {
            title: "Stan".into(),
            artist: "Eminem".into(),
            key: "TinyMusic/A/Eminem - Stan.flac".into(),
            added_at: 300,
        };
        let out = normalized(
            FavoritesManifest {
                version: 1,
                updated_at: 0,
                items: vec![first, dup, other],
            },
            999,
        );
        assert_eq!(out.version, MANIFEST_VERSION);
        assert_eq!(out.updated_at, 999);
        // 同一首歌只留一条，且保留最早加入的时间
        assert_eq!(out.items.len(), 2);
        assert_eq!(out.items[0].title, "Stan");
        let kept = out
            .items
            .iter()
            .find(|e| e.title.starts_with("5:"))
            .expect("同歌保留一条");
        assert_eq!(kept.added_at, 100);
    }

    #[test]
    fn canonical_prefers_playlist_copy_over_liked_folder() {
        let index = idx(&[
            "TinyMusic/喜欢/A - Song.flac",
            "TinyMusic/歌单/A - Song.flac",
            "TinyMusic/喜欢/B - Other.flac",
        ]);
        assert_eq!(
            index.canonical_for("Song", "A"),
            Some("TinyMusic/歌单/A - Song.flac".to_string())
        );
        // 只有兜底目录里一份时，就指向它
        assert_eq!(
            index.canonical_for("Other", "B"),
            Some("TinyMusic/喜欢/B - Other.flac".to_string())
        );
        // 按 key 找同一首歌也能拐到歌单那份
        assert_eq!(
            index.canonical_near("TinyMusic/喜欢/A - Song.flac"),
            Some("TinyMusic/歌单/A - Song.flac".to_string())
        );
    }

    #[test]
    fn lookup_falls_back_to_upload_filename() {
        // 标题里带 " - "，反解会分错歌手/歌名，精确文件名兜底
        let index = idx(&["TinyMusic/喜欢/A - B - Song.flac"]);
        let track = Track {
            provider: "netease".into(),
            id: "1".into(),
            title: "B - Song".into(),
            artist: "A".into(),
            ..Track::default()
        };
        assert_eq!(
            index.lookup(&track),
            Some("TinyMusic/喜欢/A - B - Song.flac".to_string())
        );
    }

    #[test]
    fn cache_keeps_pending_likes_and_drops_stale_rows() {
        let storage = Storage::open_in_memory().unwrap();
        let gone = Track {
            provider: "s3".into(),
            id: "TinyMusic/喜欢/gone.mp3".into(),
            title: "Gone".into(),
            artist: "X".into(),
            ..Track::default()
        };
        let keep = Track {
            provider: "s3".into(),
            id: "TinyMusic/喜欢/keep.mp3".into(),
            title: "Keep".into(),
            artist: "X".into(),
            ..Track::default()
        };
        let waiting = Track {
            provider: "netease".into(),
            id: "42".into(),
            title: "Remote".into(),
            artist: "Y".into(),
            ..Track::default()
        };
        storage.add_favorite(&gone).unwrap();
        storage.add_favorite_pending(&waiting).unwrap();

        sync_cache(&storage, std::slice::from_ref(&keep));

        let ids: Vec<String> = storage
            .list_favorites()
            .unwrap()
            .into_iter()
            .map(|t| format!("{}:{}", t.provider, t.id))
            .collect();
        assert!(ids.iter().any(|k| k == "s3:TinyMusic/喜欢/keep.mp3"));
        assert!(!ids.iter().any(|k| k == "s3:TinyMusic/喜欢/gone.mp3"));
        // 还没写进清单的平台喜欢不能被顺手删掉
        assert!(ids.iter().any(|k| k == "netease:42"));
    }

    #[test]
    fn commit_marks_local_row_synced() {
        let storage = Storage::open_in_memory().unwrap();
        let waiting = Track {
            provider: "netease".into(),
            id: "42".into(),
            title: "Song".into(),
            artist: "A".into(),
            ..Track::default()
        };
        storage.add_favorite_pending(&waiting).unwrap();
        assert_eq!(storage.list_pending_favorites().unwrap().len(), 1);

        // 平台 id 换成网盘 key（这里只验证本地镜像这一步）
        storage
            .mark_favorite_synced("netease", "42", "TinyMusic/喜欢/A - Song.flac")
            .unwrap();
        let pending = storage.list_pending_favorites().unwrap();
        assert!(pending.is_empty());
        let all = storage.list_favorites().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "TinyMusic/喜欢/A - Song.flac");
    }
}

