//! Tauri 外壳：状态装配 + IPC 命令面。业务逻辑全部在 `tinymusic-core`。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};

use tinymusic_core::download::{build_filename, build_lrc, DownloadExtras, Downloader};
use tinymusic_core::http::HttpClient;
use tinymusic_core::model::*;
use tinymusic_core::proxy::StreamProxy;
use tinymusic_core::providers::kuwo::KuwoProvider;
use tinymusic_core::s3::{S3Config, S3};
use tinymusic_core::storage::{DownloadRow, Storage};

// ---------- 设置 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub download_dir: String,
    pub default_quality: String,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,

    // ---- 网盘（S3 兼容）----
    #[serde(default)]
    pub s3_enabled: bool,
    #[serde(default)]
    pub s3_endpoint: String,
    #[serde(default = "default_region")]
    pub s3_region: String,
    #[serde(default)]
    pub s3_bucket: String,
    #[serde(default)]
    pub s3_access_key: String,
    #[serde(default)]
    pub s3_secret_key: String,
    #[serde(default = "default_s3_prefix")]
    pub s3_prefix: String,
    #[serde(default)]
    pub s3_path_style: bool,

    // ---- 窗口 / 托盘 ----
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
}

fn default_concurrency() -> u32 {
    2
}

fn default_region() -> String {
    "us-east-1".into()
}

fn default_s3_prefix() -> String {
    "TinyMusic".into()
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: dirs_download(),
            default_quality: "high".into(),
            concurrency: 2,
            s3_enabled: false,
            s3_endpoint: String::new(),
            s3_region: default_region(),
            s3_bucket: String::new(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
            s3_prefix: default_s3_prefix(),
            s3_path_style: false,
            close_to_tray: true,
        }
    }
}

fn dirs_download() -> String {
    let base = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(base)
        .join("Music")
        .join("TinyMusic")
        .to_string_lossy()
        .to_string()
}

// ---------- 应用状态 ----------

pub struct AppState {
    pub http: Arc<HttpClient>,
    pub providers: Vec<Arc<dyn MusicProvider>>,
    pub proxy: StreamProxy,
    pub storage: Arc<Storage>,
    pub downloader: Arc<Downloader>,
    pub settings: Mutex<Settings>,
    pub config_dir: PathBuf,
    pub task_seq: AtomicU64,
}

impl AppState {
    fn provider(&self, id: &str) -> Result<Arc<dyn MusicProvider>, String> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .cloned()
            .ok_or_else(|| format!("未知平台: {}", id))
    }

    fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn load_settings(dir: &std::path::Path) -> Settings {
    let p = dir.join("settings.json");
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(dir: &std::path::Path, s: &Settings) {
    if let Ok(txt) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(dir.join("settings.json"), txt);
    }
}

// ---------- 网盘辅助 ----------

fn s3_client(state: &AppState) -> Result<S3, String> {
    let s = state.settings_snapshot();
    let cfg = S3Config {
        endpoint: s.s3_endpoint.trim().to_string(),
        region: s.s3_region.trim().to_string(),
        bucket: s.s3_bucket.trim().to_string(),
        access_key: s.s3_access_key.trim().to_string(),
        secret_key: s.s3_secret_key.trim().to_string(),
        prefix: s.s3_prefix.clone(),
        path_style: s.s3_path_style,
    };
    if !cfg.is_ready() {
        return Err(
            "请先在 设置 → 网盘（S3）里填写 Endpoint / Bucket / Access Key / Secret Key".into(),
        );
    }
    Ok(S3::new(cfg, state.http.client().clone()))
}

/// 歌单在网盘里就是一个前缀目录
fn playlist_prefix(prefix: &str, playlist: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    let pl = playlist.trim().trim_matches('/');
    match (p.is_empty(), pl.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("{}/", p),
        (true, false) => format!("{}/", pl),
        (false, false) => format!("{}/{}/", p, pl),
    }
}

fn object_key(prefix: &str, playlist: &str, track: &Track, ext: &str) -> String {
    format!(
        "{}{}",
        playlist_prefix(prefix, playlist),
        build_filename(track, ext)
    )
}

fn replace_ext(key: &str, ext: &str) -> String {
    match key.rfind('.') {
        Some(i) if !key[i + 1..].contains('/') => format!("{}.{}", &key[..i], ext),
        _ => format!("{}.{}", key, ext),
    }
}

fn is_audio_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [".mp3", ".flac", ".m4a", ".ogg", ".wav"]
        .iter()
        .any(|e| lower.ends_with(e))
}

fn audio_content_type(key: &str) -> &'static str {
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

fn sniff_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(b"RIFF") && bytes.len() > 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn sanitize_playlist_name(raw: &str) -> Result<String, String> {
    let name = raw.trim().replace(['/', '\\'], "-");
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("歌单名不能为空".into());
    }
    if name.chars().count() > 60 {
        return Err("歌单名太长了（最多 60 个字符）".into());
    }
    Ok(name)
}

/// 网盘里的「喜欢」歌单目录名
const FAVORITES_PLAYLIST: &str = "喜欢";

fn favorites_object_key(prefix: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    if p.is_empty() {
        "favorites.json".into()
    } else {
        format!("{}/favorites.json", p)
    }
}

/// 把本地喜欢列表写成网盘上的 favorites.json
async fn push_favorites(state: &AppState) -> Result<(), String> {
    let client = s3_client(state)?;
    let list = state
        .storage
        .list_favorites()
        .map_err(|e| e.to_string())?;
    let key = favorites_object_key(&client.config().prefix);
    let body = serde_json::to_vec(&list).map_err(|e| e.to_string())?;
    client
        .put_object(&key, body, "application/json; charset=utf-8")
        .await
        .map_err(|e| e.to_string())
}

/// 点喜欢之后：同步 favorites.json，并把这歌下载到网盘「喜欢」目录
async fn sync_favorite_track(state: &AppState, track: &Track) -> Result<(), String> {
    push_favorites(state).await?;

    let client = s3_client(state)?;
    let settings = state.settings_snapshot();
    let q = Quality::from_str(&settings.default_quality);
    let p = state.provider(&track.provider)?;
    let info = p.resolve(&track.id, q).await.map_err(|e| e.to_string())?;

    let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
    let task_id = format!("fav-{}-{}", seq, track.id);
    state
        .storage
        .insert_download(&task_id, track, info.quality, "", 0, "running")
        .map_err(|e| e.to_string())?;

    match upload_track_to_s3(
        state,
        &client,
        track,
        &info,
        FAVORITES_PLAYLIST,
        &task_id,
        None,
    )
    .await
    {
        Ok(_) => {
            let _ = state.storage.save_s3_playlist(FAVORITES_PLAYLIST);
            Ok(())
        }
        Err(e) => {
            let _ = state
                .storage
                .update_download(&task_id, &format!("failed: {}", e), 0);
            Err(e)
        }
    }
}

/// 下载时顺带准备的封面与歌词（都允许失败，失败就只下音频）
async fn build_extras(
    http: &HttpClient,
    provider: &dyn MusicProvider,
    track: &Track,
) -> DownloadExtras {
    let mut extras = DownloadExtras::default();

    if let Some(url) = track.cover.as_deref() {
        if !url.trim().is_empty() {
            if let Ok(Ok(bytes)) =
                tokio::time::timeout(Duration::from_secs(10), http.get_bytes("img", url)).await
            {
                if !bytes.is_empty() && bytes.len() < 8 * 1024 * 1024 {
                    extras.cover_mime = sniff_image_mime(&bytes).to_string();
                    extras.cover = Some(bytes);
                }
            }
        }
    }

    // 详情接口容易限流/变慢：超时就只下音频，不阻塞整个下载
    if let Ok(Ok(detail)) =
        tokio::time::timeout(Duration::from_secs(12), provider.detail(track)).await
    {
        if !detail.lyrics.is_empty() {
            extras.lyrics_lrc = Some(build_lrc(track, &detail.lyrics));
        }
    }

    extras
}

// ---------- 命令 ----------

#[tauri::command]
fn get_providers(state: State<'_, AppState>) -> Vec<ProviderInfo> {
    state.providers.iter().map(|p| p.info()).collect()
}

#[tauri::command]
async fn search(
    state: State<'_, AppState>,
    provider: String,
    kw: String,
    page: u32,
    limit: u32,
) -> Result<SearchPage, String> {
    let p = state.provider(&provider)?;
    p.search(&kw, page, limit).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn resolve(
    state: State<'_, AppState>,
    provider: String,
    id: String,
    quality: String,
) -> Result<StreamInfo, String> {
    let p = state.provider(&provider)?;
    p.resolve(&id, Quality::from_str(&quality))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn track_detail(
    state: State<'_, AppState>,
    provider: String,
    track: Track,
) -> Result<TrackDetail, String> {
    if provider == "s3" {
        let client = s3_client(&state)?;
        let lyrics = match client.get_object(&replace_ext(&track.id, "lrc")).await {
            Ok(Some(bytes)) => parse_lrc(&String::from_utf8_lossy(&bytes)),
            _ => Vec::new(),
        };
        return Ok(TrackDetail {
            track,
            lyrics,
            preview_range_ms: None,
        });
    }
    let p = state.provider(&provider)?;
    p.detail(&track).await.map_err(|e| e.to_string())
}

/// LRC 文本 → 歌词行（同一句可以挂多个时间戳）
fn parse_lrc(text: &str) -> Vec<LyricLine> {
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

#[tauri::command]
async fn artist(
    state: State<'_, AppState>,
    provider: String,
    artist_id: String,
    name: String,
    page: u32,
) -> Result<ArtistPage, String> {
    let p = state.provider(&provider)?;
    p.artist(&artist_id, &name, page)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn album(
    state: State<'_, AppState>,
    provider: String,
    album_id: String,
    name: String,
    page: u32,
) -> Result<AlbumPage, String> {
    let p = state.provider(&provider)?;
    p.album(&album_id, &name, page)
        .await
        .map_err(|e| e.to_string())
}

/// 取链并登记到本地代理，返回可直接给 `<audio src>` 的地址
#[tauri::command]
async fn player_open(
    state: State<'_, AppState>,
    provider: String,
    id: String,
    quality: String,
) -> Result<PlayerSource, String> {
    // 网盘里的歌：给一个 6 小时有效的签名直链，不经过平台取链
    if provider == "s3" {
        let client = s3_client(&state)?;
        let url = client.presign_get(&id, 6 * 3600).map_err(|e| e.to_string())?;
        return Ok(PlayerSource {
            url,
            quality: "high".into(),
            bytes: None,
        });
    }
    let p = state.provider(&provider)?;
    let q = Quality::from_str(&quality);
    let info = p.resolve(&id, q).await.map_err(|e| e.to_string())?;
    let url = state.proxy.register(info.url.clone()).await;
    Ok(PlayerSource {
        url,
        quality: info.quality.as_str().to_string(),
        bytes: info.bytes,
    })
}

#[derive(Debug, Serialize)]
pub struct PlayerSource {
    pub url: String,
    pub quality: String,
    pub bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub task_id: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[tauri::command]
async fn download_start(
    state: State<'_, AppState>,
    track: Track,
    quality: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<String, String> {
    let provider_id = track.provider.clone();
    let p = state.provider(&provider_id)?;
    let settings = state.settings_snapshot();
    let q = Quality::from_str(quality.as_deref().unwrap_or(&settings.default_quality));

    // 先取流（同步等待，确保能给出「不可用」的明确错误）
    let info = p.resolve(&track.id, q).await.map_err(|e| e.to_string())?;

    let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
    let task_id = format!("t{}-{}", seq, track.id);
    let dir = PathBuf::from(&settings.download_dir);
    let fname = build_filename(&track, ext_for(&info.content_type));

    state
        .storage
        .insert_download(&task_id, &track, info.quality, "", 0, "running")
        .map_err(|e| e.to_string())?;

    let tid = task_id.clone();
    let dl = state.downloader.clone();
    let storage = state.storage.clone();
    let http = state.http.clone();
    let provider = p.clone();
    let track2 = track.clone();
    let quality2 = info.quality;

    tauri::async_runtime::spawn(async move {
        // 封面（嵌进 ID3）与歌词（同名 .lrc）都顺带准备好
        let extras = build_extras(&http, provider.as_ref(), &track2).await;
        let ch = on_progress.clone();
        let tid2 = tid.clone();
        let res = dl
            .run(
                &track2,
                &info,
                &dir,
                &fname,
                &extras,
                Box::new(move |downloaded, total| {
                    let _ = ch.send(Progress {
                        task_id: tid2.clone(),
                        downloaded,
                        total,
                    });
                }),
            )
            .await;

        match res {
            Ok(path) => {
                let bytes = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
                let _ = storage.update_download(&tid, "done", bytes);
                // 用最终路径覆盖记录
                let _ = storage.insert_download(
                    &tid,
                    &track2,
                    quality2,
                    &path.to_string_lossy(),
                    bytes,
                    "done",
                );
            }
            Err(e) => {
                let _ = storage.update_download(&tid, &format!("failed: {}", e), 0);
            }
        }
    });

    Ok(task_id)
}

fn ext_for(content_type: &str) -> &'static str {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("flac") {
        "flac"
    } else if ct.contains("aac") || ct.contains("mp4") {
        "m4a"
    } else if ct.contains("ogg") {
        "ogg"
    } else {
        "mp3"
    }
}

#[tauri::command]
fn download_list(state: State<'_, AppState>) -> Result<Vec<DownloadRow>, String> {
    state.storage.list_downloads().map_err(|e| e.to_string())
}

#[tauri::command]
fn download_clear(state: State<'_, AppState>) -> Result<(), String> {
    state.storage.clear_downloads().map_err(|e| e.to_string())
}

#[tauri::command]
fn favorite_add(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    track: Track,
) -> Result<(), String> {
    state
        .storage
        .add_favorite(&track)
        .map_err(|e| e.to_string())?;
    // 网络操作放后台：喜欢是即时动作，不能被下载/上传卡住
    let app2 = app.clone();
    let track2 = track.clone();
    tauri::async_runtime::spawn(async move {
        let st = app2.state::<AppState>();
        let _ = sync_favorite_track(&st, &track2).await;
    });
    Ok(())
}

#[tauri::command]
fn favorite_remove(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<(), String> {
    state
        .storage
        .remove_favorite(&provider, &id)
        .map_err(|e| e.to_string())?;
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app2.state::<AppState>();
        let _ = push_favorites(&st).await;
    });
    Ok(())
}

#[tauri::command]
fn favorite_list(state: State<'_, AppState>) -> Result<Vec<Track>, String> {
    state.storage.list_favorites().map_err(|e| e.to_string())
}

#[tauri::command]
fn favorite_check(
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<bool, String> {
    state
        .storage
        .is_favorite(&provider, &id)
        .map_err(|e| e.to_string())
}

/// 从网盘拉取 favorites.json，合并进本地（用于多设备/重装后恢复）
#[tauri::command]
async fn favorites_pull(state: State<'_, AppState>) -> Result<Vec<Track>, String> {
    let client = match s3_client(&state) {
        Ok(c) => c,
        Err(_) => return state.storage.list_favorites().map_err(|e| e.to_string()),
    };
    let key = favorites_object_key(&client.config().prefix);
    let remote = match client.get_object(&key).await.map_err(|e| e.to_string())? {
        Some(bytes) => serde_json::from_slice::<Vec<Track>>(&bytes).unwrap_or_default(),
        None => Vec::new(),
    };
    for t in &remote {
        let _ = state.storage.upsert_track(t);
        let _ = state.storage.add_favorite(t);
    }
    state.storage.list_favorites().map_err(|e| e.to_string())
}

#[tauri::command]
fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings_snapshot()
}

#[tauri::command]
fn settings_set(state: State<'_, AppState>, settings: Settings) -> Result<(), String> {
    if let Ok(mut s) = state.settings.lock() {
        *s = settings.clone();
    }
    save_settings(&state.config_dir, &settings);
    Ok(())
}

// ---------- 歌单 ----------

#[tauri::command]
async fn search_playlists(
    state: State<'_, AppState>,
    provider: String,
    kw: String,
    page: u32,
    limit: u32,
) -> Result<PlaylistPage, String> {
    let p = state.provider(&provider)?;
    p.search_playlists(&kw, page, limit)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn playlist(
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<PlaylistDetail, String> {
    let p = state.provider(&provider)?;
    p.playlist(&id).await.map_err(|e| e.to_string())
}

/// 按 uid 列出用户歌单（酷我这条接口不需要登录）
#[tauri::command]
async fn user_playlists(
    state: State<'_, AppState>,
    provider: String,
    uid: String,
    limit: Option<u32>,
) -> Result<Vec<PlaylistRef>, String> {
    if uid.trim().is_empty() {
        return Err("uid 不能为空".into());
    }
    let p = state.provider(&provider)?;
    p.user_playlists(uid.trim(), limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())
}

// ---------- 网页登录（只为拿 uid，目的就是歌单）----------

const LOGIN_LABEL: &str = "kwlogin";
/// 登录窗口尺寸（逻辑点，等于站点登录弹窗的大小）
const LOGIN_W: f64 = 372.0;
// 多出来的 32px 是标题栏，否则登录框底部会被裁掉
const LOGIN_H: f64 = 572.0;

/// 在网页里登录后，从 cookie / localStorage / sessionStorage 里翻出 uid，
/// 写进 document.title，由 Rust 侧轮询读走。
///
/// 之所以走 title 而不是 Tauri IPC：登录页是远程域名，默认拿不到 IPC 权限，
/// 而 eval 本身没有返回值通道。
const PROBE_JS: &str = r#"
(function () {
  try {
    var parts = [document.cookie || ''];
    try {
      for (var i = 0; i < localStorage.length; i++) {
        var k = localStorage.key(i);
        parts.push(k + '=' + localStorage.getItem(k));
      }
    } catch (e) {}
    try {
      for (var j = 0; j < sessionStorage.length; j++) {
        var k2 = sessionStorage.key(j);
        parts.push(k2 + '=' + sessionStorage.getItem(k2));
      }
    } catch (e) {}
    var blob = parts.join('&');
    var m = blob.match(/["']?(?:uid|userId|userid|loginUid|kwUid|user_id)["']?\s*[:=]\s*["']?(\d{4,})/i);
    if (!m) { m = blob.match(/[?&]uid=(\d{4,})/i); }
    if (m) {
      // 主通道：写进 cookie，SPA 改标题也不会丢
      try { document.cookie = 'kwprobe_uid=' + m[1] + '; path=/; domain=.kuwo.cn'; } catch (e) {}
      // 备用通道
      document.title = 'KWUID:' + m[1];
    }
  } catch (e) {}
})();
"#;

/// 首页的登录入口是 `<div class="reg_text">登录 / 注册</div>`。
/// 窗口打开后反复注入这段脚本，把登录弹窗点出来；脚本自带幂等保护，只会点一次。
const AUTO_LOGIN_JS: &str = r#"
(function () {
  try {
    if (!window.__kwStyleInjected) {
      var st = document.createElement('style');
      st.textContent = '.reg_text,.login_text,[class*="login"]{display:flex!important;' +
                       'visibility:visible!important;opacity:1!important;}' +
                       '.reg_text span{display:inline!important;}';
      document.head.appendChild(st);
      window.__kwStyleInjected = true;
    }

    // 先判断登录弹窗是否已经打开
    var open = false;
    // 1) 站点自己的登录遮罩（最准）
    var reg = document.querySelector('.reg_login');
    if (reg && reg.offsetParent !== null) open = true;
    // 2) 文本特征：弹窗底部一定有「快捷登录」/用户协议
    if (!open) {
      var tips = document.querySelectorAll('span,a,div,p');
      for (var t = 0; t < tips.length && t < 3000; t++) {
        var tx = (tips[t].textContent || '');
        if (tx.indexOf('快捷登录') >= 0 || tx.indexOf('酷我音乐用户服务协议') >= 0) {
          if (tips[t].offsetParent !== null) { open = true; break; }
        }
      }
    }
    // 3) fixed + 高 z-index + 面积够大
    if (!open) {
      var boxes = document.querySelectorAll('div');
      for (var i = 0; i < boxes.length; i++) {
        var cs = getComputedStyle(boxes[i]);
        if (cs.position !== 'fixed' || cs.display === 'none' || cs.visibility === 'hidden') continue;
        if ((parseInt(cs.zIndex, 10) || 0) < 100) continue;
        var r = boxes[i].getBoundingClientRect();
        if (r.width > 300 && r.height > 300) { open = true; break; }
      }
    }
    document.cookie = 'kwprobe_modal=' + (open ? '1' : '0') + '; path=/; domain=.kuwo.cn';
    if (open) return;

    // 给入口加一个醒目的框 + 提示（DOM 注入不受 isTrusted 限制）
    if (!window.__kwHinted) {
      var entry = document.querySelector('.reg_text');
      if (entry) {
        entry.style.setProperty('outline', '3px solid #ffd400', 'important');
        entry.style.setProperty('outline-offset', '4px', 'important');
        entry.style.setProperty('border-radius', '6px', 'important');
        var tip = document.createElement('div');
        tip.textContent = '点这里登录';
        tip.style.cssText = 'position:absolute;z-index:2147483647;background:#1b1b1b;color:#ffd400;' +
          'font:600 13px/1.4 -apple-system,sans-serif;padding:5px 9px;border-radius:6px;' +
          'pointer-events:none;white-space:nowrap;transform:translate(-50%,100%);';
        var host = entry.closest('div') || entry.parentElement || entry;
        host.style.setProperty('position', 'relative', 'important');
        host.appendChild(tip);
        tip.style.left = '50%';
        tip.style.top = '100%';
        window.__kwHinted = true;
      }
    }

    // 站点的登录入口是「开关」：连续点会把弹窗又关掉。
    // 这里最多每 2.5 秒点一次，等 hydration 完成后仍有机会补点。
    var now = Date.now();
    if (window.__kwClickedAt && now - window.__kwClickedAt < 2500) return;
    window.__kwClickedAt = now;

    function fire(el) {
      if (!el) return;
      try { el.scrollIntoView({ block: 'center' }); } catch (e) {}
      ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click'].forEach(function (t) {
        try {
          var Ev = t.indexOf('pointer') === 0 ? PointerEvent : MouseEvent;
          el.dispatchEvent(new Ev(t, { bubbles: true, cancelable: true, view: window }));
        } catch (e) {}
      });
      try { el.click(); } catch (e) {}
    }

    // 站点入口：每轮都试一次（首轮可能还在 hydration，点了不生效）
    var sels = ['.reg_text', '.login_text', '.login-btn', 'a[href*="login"]'];
    for (var k = 0; k < sels.length; k++) {
      var el = document.querySelector(sels[k]);
      if (el && el.offsetParent !== null) { fire(el); return; }
    }

    // 文本兜底
    var nodes = document.querySelectorAll('span,a,button');
    for (var j = 0; j < nodes.length && j < 4000; j++) {
      var n = nodes[j];
      if (n.children.length !== 0) continue;
      var t = (n.textContent || '').replace(/\s+/g, '');
      if (t === '登录' || t === '登录/注册') {
        if (n.offsetParent !== null) { fire(n); return; }
      }
    }
  } catch (e) {}
})();
"#;

/// 弹窗出现后调用：把页面其余部分隐藏，只留登录框，并把它钉到窗口左上角。
///
/// 之所以要隐藏而不是靠窄视口：站点在窄视口下会切到移动布局，
/// 首页「登录/注册」入口会消失，弹窗根本点不出来。
const ISOLATE_MODAL_JS: &str = r#"
(function () {
  try {
    // 实测结构：`.reg_login` 是全屏遮罩，真正的对话框是它里面的 `.inner`（360x540）。
    // 之前从「第一个 fixed 大元素」入手，抓到的是对话框内部的内容盒，
    // 结果把同级的另一半面板一起藏了。
    function findModal() {
      return document.querySelector('.reg_login .inner') ||
             document.querySelector('.reg_login') ||
             document.body;
    }

    var modal = findModal();
    if (!modal) return;
    // 逐层往上，把兄弟节点全部隐藏
    var n = modal;
    while (n && n !== document.body) {
      var p = n.parentElement;
      if (!p) break;
      for (var i = 0; i < p.children.length; i++) {
        var c = p.children[i];
        if (c !== n && c.style) {
          c.style.setProperty('display', 'none', 'important');
        }
      }
      n = p;
    }
    // 钉到左上角，让窗口裁切后正好只剩这个框
    modal.style.setProperty('position', 'fixed', 'important');
    modal.style.setProperty('left', '0', 'important');
    modal.style.setProperty('top', '0', 'important');
    modal.style.setProperty('margin', '0', 'important');
    modal.style.setProperty('transform', 'none', 'important');
    // 这是遮罩层，把它刷白，里面的登录卡片就自然成为窗口主体
    modal.style.setProperty('background', '#fff', 'important');
    // 遮罩层是它的祖先，也要刷白，否则窗口边缘会露出灰色
    var up = modal.parentElement;
    for (var k = 0; k < 6 && up && up !== document.body; k++) {
      up.style.setProperty('background', '#fff', 'important');
      up = up.parentElement;
    }
    document.documentElement.style.setProperty('background', '#fff', 'important');
    document.body.style.setProperty('background', '#fff', 'important');
    modal.style.setProperty('overflow', 'hidden', 'important');
    modal.style.setProperty('border-radius', '12px', 'important');
    document.documentElement.style.setProperty('background', '#fff', 'important');
    document.body.style.setProperty('background', '#fff', 'important');
    document.body.style.setProperty('overflow', 'hidden', 'important');
    window.__kwIsolated = true;
  } catch (e) {}
})();
"#;

/// 把窗口居中到它所在的显示器。
///
/// 这里全部用**逻辑坐标**：`set_position` 收到 PhysicalPosition 会再乘一次
/// 缩放因子（这台机器是 2x），窗口会被推到屏幕外；换成 LogicalPosition 才对。
fn place_centered(w: &tauri::WebviewWindow, logical_w: f64, logical_h: f64) {
    // 关键：要跟随**主窗口所在的显示器**，而不是主显示器。
    // 用 `w.current_monitor()` 会拿到主屏（窗口刚创建还没显示），
    // 主应用在副屏时登录框就跑到另一块屏去了。
    let main = w.app_handle().get_webview_window("main");
    let mon = main
        .as_ref()
        .and_then(|m| m.current_monitor().ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten());
    let Some(mon) = mon else { return };

    let sf = mon.scale_factor();
    let mp = mon.position(); // 物理，可能是负值（副屏在主屏左侧）
    let ms = mon.size(); // 物理
    let win_w = logical_w * sf;
    let win_h = logical_h * sf;

    // 先按物理算「屏幕内居中」，再换算成逻辑坐标回填
    let px = mp.x as f64 + (ms.width as f64 - win_w) / 2.0;
    let py = mp.y as f64 + (ms.height as f64 - win_h) / 2.0;
    let _ = w.set_position(tauri::LogicalPosition::new(px / sf, py / sf));
}

#[tauri::command]
async fn login_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = tauri::Url::parse("https://www.kuwo.cn/").map_err(|e| e.to_string())?;
    let popup_app = app.clone();
    let w = tauri::WebviewWindowBuilder::new(&app, LOGIN_LABEL, tauri::WebviewUrl::External(url))
        .title("酷我登录")
        // 普通窗口：保留标题栏和关闭按钮
        .always_on_top(true)
        .resizable(false)
        // 先按桌面宽度创建（此时窗口是隐藏的，用户看不到），
        // 否则窄视口下站点会切移动布局，「登录/注册」入口根本不存在，弹窗点不出来

        // 直接显示：网页登录入口只认真实点击，合成事件点不开，
        // 所以先把页面亮出来，用户点一次右上角「登录/注册」后我们再收成登录框
        .visible(false)
        .inner_size(1280.0, 800.0)
        .on_new_window(move |url, features| {
            // QQ / 微信登录会 window.open 一个扫码页；WKWebView 默认弹不出来，这里显式建一个
            static POPUP_SEQ: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let seq = POPUP_SEQ.fetch_add(1, Ordering::Relaxed);
            let label = format!("kwlogin-pop-{seq}");
            let builder = tauri::WebviewWindowBuilder::new(
                &popup_app,
                &label,
                tauri::WebviewUrl::External(url.clone()),
            )
            .window_features(features)
            .always_on_top(true)
            .title("酷我登录")
            .on_document_title_changed(|window, title| {
                let _ = window.set_title(&title);
            });
            match builder.build() {
                Ok(window) => tauri::webview::NewWindowResponse::Create { window },
                Err(_) => tauri::webview::NewWindowResponse::Deny,
            }
        })
        .build()
        .map_err(|e| e.to_string())?;

    place_centered(&w, 1280.0, 800.0);
    let _ = w.show();
    let _ = w.set_focus();

    // 后台协程：把弹窗点出来，并在网页弹窗被关掉时把这个窗口一起关掉。
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut isolated = false;
        // 只有真的见过网页弹窗，"消失"才算用户关掉；
        // 否则兜底显示出来的窗口会在同一轮里被这条规则立刻关掉
        let mut ever_modal = false;
        let mut closed_streak = 0;
        let mut misses = 0u8;
        for i in 0..160 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let Some(w) = handle.get_webview_window(LOGIN_LABEL) else {
                // 连续取不到才认为窗口已经被关掉
                misses += 1;
                if misses > 5 {
                        return;
                }
                continue;
            };
            misses = 0;
            // 前 20 秒负责把弹窗点出来。
            // eval 失败不能 return：窗口刚建好时页面还没加载完，eval 必然报错，
            // 一旦在这里退出，后面的 show() 就永远执行不到（点了没反应）。
            if i < 100 {
                let _ = w.eval(AUTO_LOGIN_JS);
            }
            let cookies = w.cookies().unwrap_or_default();
            let mut modal_open = false;
            let mut has_uid = false;
            for c in &cookies {
                match c.name() {
                    "kwprobe_modal" => modal_open = c.value() == "1",
                    "kwprobe_uid" => has_uid = !c.value().is_empty(),
                    name if name.to_lowercase().contains("uid") => {
                        let v = c.value();
                        if v.len() >= 4 && v.chars().all(|ch| ch.is_ascii_digit()) {
                            has_uid = true;
                        }
                    }
                    _ => {}
                }
            }

            if modal_open && !isolated {
                // 弹窗出来了：隐藏页面其余部分，窗口收成弹窗大小，居中，然后才显示
                let _ = w.eval(ISOLATE_MODAL_JS);
                // 缩到登录框尺寸再显示：用户全程看不到「宽」的那一帧。
                // resizable(false) 的窗口会忽略 set_size，所以先解开再锁回去。
                let _ = w.set_resizable(true);
                let _ = w.set_size(tauri::LogicalSize::new(LOGIN_W, LOGIN_H));
                let _ = w.set_resizable(false);
                place_centered(&w, LOGIN_W, LOGIN_H);
                let _ = w.show();
                let _ = w.set_focus();
                let _ = w.set_title("酷我登录");
                isolated = true;
                ever_modal = true;
                closed_streak = 0;
                continue;
            }
            if isolated && modal_open {
                // 站点可能重渲染，补一次隔离
                let _ = w.eval(ISOLATE_MODAL_JS);
            }

            if has_uid {
                close_login_popups(&handle);
                return; // 登录成功，交给前端读取 uid
            }

            if ever_modal && !modal_open {
                // 弹窗被关掉（点 X 或登录后自动收）——把窗口一起收起
                closed_streak += 1;
                if closed_streak >= 1 {
                    let _ = w.close();
                        return;
                }
            } else {
                closed_streak = 0;
            }
        }
    });

    Ok(())
}

/// 登录窗口是否已经显示（前端用它结束按钮的「正在打开…」）
#[tauri::command]
fn login_visible(app: tauri::AppHandle) -> bool {
    app.get_webview_window(LOGIN_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 返回 `Ok(Some(uid))` 表示已经拿到；`Ok(None)` 表示还没登录或还没探测到。
#[tauri::command]
async fn login_probe(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let w = app
        .get_webview_window(LOGIN_LABEL)
        .ok_or_else(|| "登录窗口未打开".to_string())?;

    let _ = w.eval(PROBE_JS);
    tokio::time::sleep(std::time::Duration::from_millis(350)).await;

    let cookies = w.cookies().unwrap_or_default();

    // 1) 我们注入的专用 cookie —— 最可靠
    for c in &cookies {
        if c.name() == "kwprobe_uid" {
            let v = c.value().to_string();
            if v.len() >= 4 && v.chars().all(|ch| ch.is_ascii_digit()) {
                return Ok(Some(v));
            }
        }
    }

    // 2) 站点自身的 uid/userid cookie（读得到 HttpOnly）
    for c in &cookies {
        let name = c.name().to_lowercase();
        let val = c.value().to_string();
        if (name.contains("uid") || name.contains("userid"))
            && val.len() >= 4
            && val.chars().all(|ch| ch.is_ascii_digit())
        {
            return Ok(Some(val));
        }
    }

    // 3) 标题备用通道
    if let Ok(title) = w.title() {
        if let Some(rest) = title.strip_prefix("KWUID:") {
            let uid: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            if uid.len() >= 4 {
                return Ok(Some(uid));
            }
        }
    }
    Ok(None)
}

/// 登录过程中弹出的扫码窗口（QQ / 微信等）
fn close_login_popups(app: &tauri::AppHandle) {
    for (label, w) in app.webview_windows() {
        if label.starts_with("kwlogin-pop") {
            let _ = w.close();
        }
    }
}

fn clear_kuwo_cookies(w: &tauri::WebviewWindow) {
    if let Ok(cookies) = w.cookies() {
        for c in cookies {
            let domain = c.domain().unwrap_or("");
            if !domain.contains("kuwo.cn") && !c.name().starts_with("kw") {
                continue;
            }
            let mut b = tauri::webview::cookie::Cookie::build((
                c.name().to_string(),
                String::new(),
            ))
            .path(c.path().unwrap_or("/").to_string())
            .secure(c.secure().unwrap_or(true))
            .max_age(tauri::webview::cookie::time::Duration::ZERO);
            if !domain.is_empty() {
                b = b.domain(domain.to_string());
            }
            let ck = b.build();
            let _ = w.delete_cookie(ck);
        }
    }
}

/// 退出登录时清掉酷我网页登录态（cookie + localStorage），否则会一直沿用上一个账号
#[tauri::command]
async fn login_purge(app: tauri::AppHandle) -> Result<(), String> {
    close_login_popups(&app);

    let existing = app.get_webview_window(LOGIN_LABEL);
    let (w, created) = match existing {
        Some(w) => (w, false),
        None => {
            let url =
                tauri::Url::parse("https://www.kuwo.cn/").map_err(|e| e.to_string())?;
            let w = tauri::WebviewWindowBuilder::new(
                &app,
                "kwlogin-purge",
                tauri::WebviewUrl::External(url),
            )
            .visible(false)
            .inner_size(420.0, 320.0)
            .build()
            .map_err(|e| e.to_string())?;
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
            (w, true)
        }
    };

    let _ = w.eval(
        "try{localStorage.clear();sessionStorage.clear();document.cookie='';}catch(e){}",
    );
    clear_kuwo_cookies(&w);

    if created {
        let _ = w.close();
    }
    Ok(())
}

/// 让窗口标题栏跟随应用主题；auto 时传 None，交还给系统
#[tauri::command]
fn set_window_theme(window: tauri::WebviewWindow, theme: Option<String>) -> Result<(), String> {
    let t = match theme.as_deref() {
        Some("light") => Some(tauri::Theme::Light),
        Some("dark") => Some(tauri::Theme::Dark),
        _ => None,
    };
    window.set_theme(t).map_err(|e| e.to_string())
}

#[tauri::command]
fn login_close(app: tauri::AppHandle) -> Result<(), String> {
    close_login_popups(&app);
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 账号「喜欢」歌单
#[tauri::command]
async fn favorites(
    state: State<'_, AppState>,
    provider: String,
    uid: String,
) -> Result<PlaylistDetail, String> {
    if uid.trim().is_empty() {
        return Err("请先在账号页登录或填写 uid".into());
    }
    let p = state.provider(&provider)?;
    p.favorites(uid.trim()).await.map_err(|e| e.to_string())
}

/// 批量下载：顺序执行，避免同时对 CDN 发起过多请求
#[tauri::command]
async fn download_batch(
    state: State<'_, AppState>,
    tracks: Vec<Track>,
    quality: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<Vec<String>, String> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }
    let settings = state.settings_snapshot();
    let q = Quality::from_str(quality.as_deref().unwrap_or(&settings.default_quality));
    let dir = PathBuf::from(&settings.download_dir);

    let mut ids = Vec::new();
    let mut failed = 0usize;
    let total_n = tracks.len();

    for (i, track) in tracks.iter().enumerate() {
        let p = state.provider(&track.provider)?;
        // 进度先报「第 i 首 / 共 n 首」的起步态，界面能看出在动
        let _ = on_progress.send(Progress {
            task_id: format!("batch-{}-{}", i, track.id),
            downloaded: i as u64,
            total: Some(total_n as u64),
        });

        let info = match p.resolve(&track.id, q).await {
            Ok(v) => v,
            Err(_) => {
                failed += 1;
                continue;
            }
        };

        let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("b{}-{}", seq, track.id);
        let fname = build_filename(track, ext_for(&info.content_type));
        state
            .storage
            .insert_download(&task_id, track, info.quality, "", 0, "running")
            .map_err(|e| e.to_string())?;

        let extras = build_extras(&state.http, p.as_ref(), track).await;
        let ch = on_progress.clone();
        let tid = task_id.clone();
        let res = state
            .downloader
            .run(
                track,
                &info,
                &dir,
                &fname,
                &extras,
                Box::new(move |d, _t| {
                    let _ = ch.send(Progress {
                        task_id: tid.clone(),
                        downloaded: d,
                        total: None,
                    });
                }),
            )
            .await;

        match res {
            Ok(path) => {
                let bytes = std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0);
                let _ = state.storage.insert_download(
                    &task_id,
                    track,
                    info.quality,
                    &path.to_string_lossy(),
                    bytes,
                    "done",
                );
                ids.push(task_id);
            }
            Err(e) => {
                let _ = state
                    .storage
                    .update_download(&task_id, &format!("failed: {}", e), 0);
                failed += 1;
            }
        }
    }

    let _ = on_progress.send(Progress {
        task_id: "batch-done".into(),
        downloaded: ids.len() as u64,
        total: Some(total_n as u64),
    });
    if ids.is_empty() && failed > 0 {
        return Err(format!("{} 首全部失败（可能都不可用）", failed));
    }
    Ok(ids)
}

#[tauri::command]
fn saved_playlist_add(state: State<'_, AppState>, playlist: PlaylistRef) -> Result<(), String> {
    state.storage.save_playlist(&playlist).map_err(|e| e.to_string())
}

#[tauri::command]
fn saved_playlist_remove(
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<(), String> {
    state
        .storage
        .remove_playlist(&provider, &id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn saved_playlist_list(state: State<'_, AppState>) -> Result<Vec<PlaylistRef>, String> {
    state.storage.list_saved_playlists().map_err(|e| e.to_string())
}

#[tauri::command]
fn saved_playlist_check(
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<bool, String> {
    state
        .storage
        .is_saved_playlist(&provider, &id)
        .map_err(|e| e.to_string())
}

// ---------- 账号 ----------

#[tauri::command]
fn provider_features(
    state: State<'_, AppState>,
    provider: String,
) -> Result<ProviderFeatures, String> {
    let p = state.provider(&provider)?;
    Ok(p.features())
}

#[tauri::command]
fn auth_capabilities(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<AuthCapability>, String> {
    let p = state.provider(&provider)?;
    Ok(p.auth_capabilities())
}

/// 统一登录入口。`method` ∈ password | cookie | qr_code。
/// 平台声明不支持的登录方式会立刻返回带原因的 Unsupported 错误。
#[tauri::command]
async fn login(
    state: State<'_, AppState>,
    provider: String,
    method: String,
    user: Option<String>,
    password: Option<String>,
    cookie: Option<String>,
) -> Result<Session, String> {
    let p = state.provider(&provider)?;

    // 先查能力声明，避免把注定失败的请求发出去
    let caps = p.auth_capabilities();
    let wanted = match method.as_str() {
        "password" => AuthMethod::Password,
        "cookie" => AuthMethod::Cookie,
        "qr_code" => AuthMethod::QrCode,
        other => return Err(format!("未知登录方式: {}", other)),
    };
    if let Some(c) = caps.iter().find(|c| c.method == wanted) {
        if !c.supported {
            return Err(c
                .reason
                .clone()
                .unwrap_or_else(|| "该登录方式不可用".to_string()));
        }
    }

    let session = match wanted {
        AuthMethod::Password => {
            p.login_password(user.as_deref().unwrap_or(""), password.as_deref().unwrap_or(""))
                .await
        }
        AuthMethod::Cookie => p.login_cookie(cookie.as_deref().unwrap_or("")).await,
        AuthMethod::QrCode => Err(tinymusic_core::Error::Unsupported(
            "扫码登录暂未接入".into(),
        )),
    }
    .map_err(|e| e.to_string())?;

    state
        .storage
        .save_session(&session)
        .map_err(|e| e.to_string())?;
    Ok(session)
}

#[tauri::command]
async fn logout(state: State<'_, AppState>, provider: String) -> Result<(), String> {
    let p = state.provider(&provider)?;
    if let Some(sess) = state.storage.get_session(&provider).map_err(|e| e.to_string())? {
        let _ = p.logout(&sess).await;
    }
    state
        .storage
        .delete_session(&provider)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn session_get(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Option<Session>, String> {
    state.storage.get_session(&provider).map_err(|e| e.to_string())
}

/// 当前账号的歌单（未登录或平台不支持时返回可读错误）
#[tauri::command]
async fn my_playlists(
    state: State<'_, AppState>,
    provider: String,
) -> Result<Vec<PlaylistRef>, String> {
    let p = state.provider(&provider)?;
    let sess = state
        .storage
        .get_session(&provider)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "尚未登录".to_string())?;
    p.my_playlists(&sess).await.map_err(|e| e.to_string())
}

#[tauri::command]
fn open_download_dir(state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.settings_snapshot().download_dir;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
    Ok(())
}

// ---------- 网盘（S3）----------

#[derive(Debug, Clone, Serialize)]
pub struct S3PlaylistInfo {
    pub name: String,
    pub count: u32,
    /// 第一首歌的封面直链（决定卡片长什么样，可以为空）
    #[serde(default)]
    pub cover: Option<String>,
}

#[tauri::command]
async fn s3_test(state: State<'_, AppState>) -> Result<String, String> {
    let s3 = s3_client(&state)?;
    s3.test().await.map_err(|e| e.to_string())
}

/// 本地记录的歌单 + 网盘上实际存在的曲目数
#[tauri::command]
async fn s3_playlists(state: State<'_, AppState>) -> Result<Vec<S3PlaylistInfo>, String> {
    let names = state
        .storage
        .list_s3_playlists()
        .map_err(|e| e.to_string())?;
    let client = s3_client(&state).ok();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut count = 0u32;
        let mut cover = None;
        if let Some(c) = client.as_ref() {
            let prefix = playlist_prefix(&c.config().prefix, &name);
            if let Ok(keys) = c.list_keys(&prefix).await {
                count = keys.iter().filter(|k| is_audio_key(k)).count() as u32;
                let lower: Vec<(String, String)> = keys
                    .iter()
                    .map(|k| (k.to_lowercase(), k.clone()))
                    .collect();
                if let Some(first) = keys.iter().find(|k| is_audio_key(k)) {
                    let stem = first.rsplit_once('.').map(|(a, _)| a).unwrap_or(first.as_str());
                    if let Some(img) = find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"]) {
                        cover = c.presign_get(&img, 6 * 3600).ok();
                    }
                }
            }
        }
        out.push(S3PlaylistInfo { name, count, cover });
    }
    Ok(out)
}

/// 网盘歌单里的一个音频文件（`track.id` 就是对象 key）
#[derive(Debug, Clone, Serialize)]
pub struct S3TrackRow {
    pub key: String,
    pub track: Track,
}

fn parse_track_filename(stem: &str) -> (String, String) {
    let name = stem.rsplit('/').next().unwrap_or(stem).trim();
    match name.split_once(" - ") {
        Some((artist, title)) => (artist.trim().to_string(), title.trim().to_string()),
        None => (String::new(), name.to_string()),
    }
}

/// 在对象列表里找同名不同后缀的兄弟文件（歌词/封面）
fn find_sibling(keys: &[(String, String)], stem: &str, exts: &[&str]) -> Option<String> {
    for ext in exts {
        let target = format!("{}.{}", stem, ext).to_lowercase();
        if let Some((_, key)) = keys.iter().find(|(lower, _)| *lower == target) {
            return Some(key.clone());
        }
    }
    None
}

/// 列出某个网盘歌单里的歌曲（音频 + 同名歌词/封面）
#[tauri::command]
async fn s3_playlist_tracks(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<S3TrackRow>, String> {
    let client = s3_client(&state)?;
    let folder = playlist_prefix(&client.normalized_prefix(), &name);
    let keys = client
        .list_keys(&folder)
        .await
        .map_err(|e| e.to_string())?;
    let lower: Vec<(String, String)> = keys
        .iter()
        .map(|k| (k.to_lowercase(), k.clone()))
        .collect();

    let mut rows = Vec::new();
    for key in keys.iter().filter(|k| is_audio_key(k)) {
        let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key.as_str());
        let (artist, title) = parse_track_filename(stem);
        let cover = find_sibling(&lower, stem, &["jpg", "jpeg", "png", "webp"])
            .and_then(|k| client.presign_get(&k, 6 * 3600).ok());

        rows.push(S3TrackRow {
            key: key.clone(),
            track: Track {
                provider: "s3".into(),
                id: key.clone(),
                title,
                artist,
                cover,
                availability: Availability::Available,
                ..Track::default()
            },
        });
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(rows)
}

/// 删除网盘歌单：连同云端文件一起删
#[tauri::command]
async fn s3_playlist_purge(state: State<'_, AppState>, name: String) -> Result<(), String> {
    let name = sanitize_playlist_name(&name)?;
    if name == FAVORITES_PLAYLIST {
        return Err("「喜欢」由红心管理，不能在这里删除".into());
    }
    let client = s3_client(&state)?;
    let folder = playlist_prefix(&client.normalized_prefix(), &name);
    let keys = client
        .list_keys(&folder)
        .await
        .map_err(|e| e.to_string())?;
    for key in keys {
        client
            .delete_object(&key)
            .await
            .map_err(|e| format!("删除 {} 失败：{}", key, e))?;
    }
    state
        .storage
        .remove_s3_playlist(&name)
        .map_err(|e| e.to_string())
}

/// 重命名歌单：按前缀把对象一个个搬到新目录
#[tauri::command]
async fn s3_playlist_rename(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> Result<String, String> {
    let old = sanitize_playlist_name(&old)?;
    let new = sanitize_playlist_name(&new)?;
    if old == FAVORITES_PLAYLIST {
        return Err("「喜欢」不能重命名".into());
    }
    if new == FAVORITES_PLAYLIST {
        return Err("「喜欢」是保留名".into());
    }
    if old == new {
        return Ok(new);
    }
    let existing = state
        .storage
        .list_s3_playlists()
        .map_err(|e| e.to_string())?;
    if existing.iter().any(|p| p == &new) {
        return Err(format!("已经有叫「{}」的歌单了", new));
    }

    let client = s3_client(&state)?;
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
            .move_object(&key, &format!("{}{}", new_folder, suffix))
            .await
            .map_err(|e| format!("移动 {} 失败：{}", key, e))?;
    }

    state
        .storage
        .remove_s3_playlist(&old)
        .map_err(|e| e.to_string())?;
    state
        .storage
        .save_s3_playlist(&new)
        .map_err(|e| e.to_string())?;
    Ok(new)
}

/// 删除歌单里的单曲（音频 + 同名歌词/封面）
#[tauri::command]
async fn s3_track_delete(state: State<'_, AppState>, key: String) -> Result<(), String> {
    let client = s3_client(&state)?;
    client
        .delete_object(&key)
        .await
        .map_err(|e| e.to_string())?;
    let stem = key.rsplit_once('.').map(|(a, _)| a).unwrap_or(key.as_str());
    for ext in ["lrc", "jpg", "jpeg", "png", "webp"] {
        let _ = client
            .delete_object(&format!("{}.{}", stem, ext))
            .await;
    }
    Ok(())
}

/// 按拖拽后的顺序保存歌单排序
#[tauri::command]
fn s3_playlists_reorder(state: State<'_, AppState>, names: Vec<String>) -> Result<(), String> {
    state
        .storage
        .reorder_s3_playlists(&names)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn s3_playlist_create(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let name = sanitize_playlist_name(&name)?;
    state
        .storage
        .save_s3_playlist(&name)
        .map_err(|e| e.to_string())?;
    Ok(name)
}



/// 下载一首到临时目录，再上传音频 + 歌词 + 封面，最后清掉临时文件
async fn upload_track_to_s3(
    state: &AppState,
    s3: &S3,
    track: &Track,
    info: &StreamInfo,
    playlist: &str,
    task_id: &str,
    on_progress: Option<&Channel<Progress>>,
) -> Result<String, String> {
    let ext = ext_for(&info.content_type);
    let key = object_key(&s3.config().prefix, playlist, track, ext);

    let provider = state.provider(&track.provider)?;
    let extras = build_extras(&state.http, provider.as_ref(), track).await;

    let ch = on_progress.cloned();
    let tid = task_id.to_string();
    // 直接在内存里下载 + 打标签，不写临时文件：避免被杀软误判为「加密/删除用户音乐」
    let bytes = state
        .downloader
        .fetch_bytes(
            track,
            info,
            &extras,
            Box::new(move |d, t| {
                if let Some(ch) = &ch {
                    let _ = ch.send(Progress {
                        task_id: tid.clone(),
                        downloaded: d,
                        total: t,
                    });
                }
            }),
        )
        .await
        .map_err(|e| e.to_string())?;
    let size = bytes.len() as i64;
    s3.put_object(&key, bytes, audio_content_type(&key))
        .await
        .map_err(|e| e.to_string())?;

    if let Some(lrc) = &extras.lyrics_lrc {
        let lrc_key = replace_ext(&key, "lrc");
        s3.put_object(
            &lrc_key,
            lrc.clone().into_bytes(),
            "text/plain; charset=utf-8",
        )
        .await
        .map_err(|e| e.to_string())?;
    }

    if let Some(cover) = &extras.cover {
        let cover_key = replace_ext(&key, "jpg");
        let mime = if extras.cover_mime.trim().is_empty() {
            "image/jpeg"
        } else {
            extras.cover_mime.as_str()
        };
        s3.put_object(&cover_key, cover.clone(), mime)
            .await
            .map_err(|e| e.to_string())?;
    }

    let path_str = format!("s3://{}/{}", s3.config().bucket.trim(), key);
    state
        .storage
        .insert_download(task_id, track, info.quality, &path_str, size, "done")
        .map_err(|e| e.to_string())?;

    Ok(key)
}

#[tauri::command]
async fn s3_download(
    state: State<'_, AppState>,
    track: Track,
    quality: Option<String>,
    playlist: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<String, String> {
    let settings = state.settings_snapshot();
    let q = Quality::from_str(quality.as_deref().unwrap_or(&settings.default_quality));
    let s3 = s3_client(&state)?;

    let raw_playlist = playlist.unwrap_or_default();
    let playlist = if raw_playlist.trim().is_empty() {
        String::new()
    } else {
        sanitize_playlist_name(&raw_playlist)?
    };

    let p = state.provider(&track.provider)?;
    let info = p.resolve(&track.id, q).await.map_err(|e| e.to_string())?;

    let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
    let task_id = format!("s3-{}-{}", seq, track.id);
    state
        .storage
        .insert_download(&task_id, &track, info.quality, "", 0, "running")
        .map_err(|e| e.to_string())?;

    match upload_track_to_s3(
        &state,
        &s3,
        &track,
        &info,
        &playlist,
        &task_id,
        Some(&on_progress),
    )
    .await
    {
        Ok(key) => {
            if !playlist.is_empty() {
                let _ = state.storage.save_s3_playlist(&playlist);
            }
            Ok(key)
        }
        Err(e) => {
            let _ = state
                .storage
                .update_download(&task_id, &format!("failed: {}", e), 0);
            Err(e)
        }
    }
}

#[tauri::command]
async fn s3_download_batch(
    state: State<'_, AppState>,
    tracks: Vec<Track>,
    quality: Option<String>,
    playlist: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<Vec<String>, String> {
    if tracks.is_empty() {
        return Ok(Vec::new());
    }
    let settings = state.settings_snapshot();
    let q = Quality::from_str(quality.as_deref().unwrap_or(&settings.default_quality));
    let s3 = s3_client(&state)?;

    let raw_playlist = playlist.unwrap_or_default();
    let playlist = if raw_playlist.trim().is_empty() {
        String::new()
    } else {
        sanitize_playlist_name(&raw_playlist)?
    };

    let total = tracks.len();
    let mut keys = Vec::new();
    let mut failed = 0usize;

    for (i, track) in tracks.iter().enumerate() {
        let _ = on_progress.send(Progress {
            task_id: format!("batch-{}-{}", i, track.id),
            downloaded: i as u64,
            total: Some(total as u64),
        });

        let p = state.provider(&track.provider)?;
        let info = match p.resolve(&track.id, q).await {
            Ok(v) => v,
            Err(_) => {
                failed += 1;
                continue;
            }
        };

        let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("s3b-{}-{}", seq, track.id);
        if let Err(e) = state
            .storage
            .insert_download(&task_id, track, info.quality, "", 0, "running")
        {
            return Err(e.to_string());
        }

        match upload_track_to_s3(&state, &s3, track, &info, &playlist, &task_id, None).await {
            Ok(k) => keys.push(k),
            Err(e) => {
                failed += 1;
                let _ = state
                    .storage
                    .update_download(&task_id, &format!("failed: {}", e), 0);
            }
        }
    }

    let _ = on_progress.send(Progress {
        task_id: "batch-done".into(),
        downloaded: keys.len() as u64,
        total: Some(total as u64),
    });

    if !playlist.is_empty() {
        let _ = state.storage.save_s3_playlist(&playlist);
    }
    if keys.is_empty() && failed > 0 {
        return Err(format!(
            "{} 首全部失败（可能都不可用，或网盘配置有问题）",
            failed
        ));
    }
    Ok(keys)
}

// ---------- 歌单一键导入网盘 ----------

#[derive(Debug, Clone, Serialize)]
pub struct S3ImportResult {
    pub playlist: String,
    pub total: u32,
    pub uploaded: u32,
    pub failed: u32,
}

/// 把一个平台歌单整张导入网盘：逐首取流 → 下载（含封面/歌词）→ 上传。
///
/// 目标歌单（桶内目录）默认沿用平台歌单名，也可以在导入弹窗里改。
#[tauri::command]
async fn s3_import_playlist(
    state: State<'_, AppState>,
    provider: String,
    playlist_id: String,
    name: String,
    target: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<S3ImportResult, String> {
    let p = state.provider(&provider)?;
    let s3 = s3_client(&state)?;

    let detail = p
        .playlist(&playlist_id)
        .await
        .map_err(|e| e.to_string())?;
    let tracks = detail.tracks;
    let total = tracks.len();
    if total == 0 {
        return Err("这个歌单里没有曲目".into());
    }

    let target_name = match target {
        Some(t) if !t.trim().is_empty() => sanitize_playlist_name(&t)?,
        _ => sanitize_playlist_name(&name).unwrap_or_else(|_| "未命名歌单".to_string()),
    };

    let settings = state.settings_snapshot();
    let q = Quality::from_str(&settings.default_quality);

    let mut uploaded = 0u32;
    let mut failed = 0u32;

    for (i, track) in tracks.iter().enumerate() {
        let _ = on_progress.send(Progress {
            task_id: format!("batch-{}-{}", i, track.id),
            downloaded: i as u64,
            total: Some(total as u64),
        });

        let info = match p.resolve(&track.id, q).await {
            Ok(v) => v,
            Err(_) => {
                failed += 1;
                continue;
            }
        };

        let seq = state.task_seq.fetch_add(1, Ordering::Relaxed);
        let task_id = format!("imp-{}-{}", seq, track.id);
        if let Err(e) = state
            .storage
            .insert_download(&task_id, track, info.quality, "", 0, "running")
        {
            return Err(e.to_string());
        }

        match upload_track_to_s3(&state, &s3, track, &info, &target_name, &task_id, None).await {
            Ok(_) => uploaded += 1,
            Err(e) => {
                failed += 1;
                let _ = state
                    .storage
                    .update_download(&task_id, &format!("failed: {}", e), 0);
            }
        }
    }

    let _ = on_progress.send(Progress {
        task_id: "batch-done".into(),
        downloaded: uploaded as u64,
        total: Some(total as u64),
    });

    if uploaded == 0 {
        return Err(format!(
            "{} 首全部失败（可能都不可用，或网盘配置有问题）",
            failed
        ));
    }

    let _ = state.storage.save_s3_playlist(&target_name);
    Ok(S3ImportResult {
        playlist: target_name,
        total: total as u32,
        uploaded,
        failed,
    })
}

// ---------- 托盘 ----------

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主界面", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "播放 / 暂停", true, None::<&str>)?;
    let prev_i = MenuItem::with_id(app, "prev", "上一首", true, None::<&str>)?;
    let next_i = MenuItem::with_id(app, "next", "下一首", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 TinyMusic", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[&show, &sep1, &toggle, &prev_i, &next_i, &sep2, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("TinyMusic")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "toggle" => {
                let _ = app.emit("tray://action", "toggle");
            }
            "prev" => {
                let _ = app.emit("tray://action", "prev");
            }
            "next" => {
                let _ = app.emit("tray://action", "next");
            }
            "quit" => app.exit(0),
            _ => {}
        });

    // 优先用打包内的 32x32 图标：dev 模式下 default_window_icon 有时是空的，
    // 那样 macOS 菜单栏会看不到托盘入口。
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))
        .ok()
        .or_else(|| app.default_window_icon().cloned());
    if let Some(icon) = icon {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// 前端主动把窗口收进托盘（设置页/快捷键都能用）
#[tauri::command]
fn hide_to_tray(app: tauri::AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    w.hide().map_err(|e| e.to_string())
}

// ---------- 启动 ----------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("tinymusic"));
            let _ = std::fs::create_dir_all(&config_dir);

            // 旧版本会在 s3tmp 里落临时 mp3；现在上传走内存，不再产生文件。
            // 这里只尝试删空目录，避免递归删媒体文件被杀软当成可疑行为。
            let _ = std::fs::remove_dir(config_dir.join("s3tmp"));

            let http = Arc::new(HttpClient::new(700, 3));
            let proxy = tauri::async_runtime::block_on(StreamProxy::start(http.clone()))
                .expect("启动本地流代理失败");
            println!("[tinymusic] 流代理: {}", proxy.base_url());

            let providers: Vec<Arc<dyn MusicProvider>> =
                vec![Arc::new(KuwoProvider::with_http(http.clone()))];

            let storage = Arc::new(
                Storage::open(config_dir.join("tinymusic.db")).expect("打开数据库失败"),
            );
            let settings = load_settings(&config_dir);

            app.manage(AppState {
                http: http.clone(),
                providers,
                proxy,
                storage,
                downloader: Arc::new(Downloader::new(http)),
                settings: Mutex::new(settings),
                config_dir,
                task_seq: AtomicU64::new(1),
            });

            // 托盘图标 + 右键菜单：关窗口后还能控制播放
            build_tray(app)?;
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .on_window_event(|window, event| {
            // 只有主窗口「关闭」才收进托盘；登录窗口该关就关
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<AppState>();
                if state.settings_snapshot().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    // 关掉「最小化到托盘」时，关闭窗口就是退出，
                    // 否则 macOS 会留下一个没有窗口也没有托盘的进程。
                    let app = window.app_handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(120));
                        app.exit(0);
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_providers,
            search,
            resolve,
            track_detail,
            artist,
            album,
            player_open,
            download_start,
            download_list,
            download_clear,
            favorite_add,
            favorite_remove,
            favorite_list,
            favorite_check,
            favorites_pull,
            settings_get,
            settings_set,
            open_download_dir,
            search_playlists,
            user_playlists,
            login_open,
            login_visible,
            login_purge,
            set_window_theme,
            login_probe,
            login_close,
            favorites,
            download_batch,
            playlist,
            saved_playlist_add,
            saved_playlist_remove,
            saved_playlist_list,
            saved_playlist_check,
            provider_features,
            auth_capabilities,
            login,
            logout,
            session_get,
            my_playlists,
            s3_test,
            s3_playlists,
            s3_playlist_create,
            s3_playlists_reorder,
            s3_playlist_purge,
            s3_playlist_rename,
            s3_playlist_tracks,
            s3_track_delete,
            s3_download,
            s3_download_batch,
            s3_import_playlist,
            hide_to_tray,
        ])
        .build(tauri::generate_context!())
        .expect("Tauri 启动失败")
        .run(|_app, _event| {
            // macOS：点 Dock 图标把隐藏的窗口叫回来
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                show_main(_app);
            }
        });
}
