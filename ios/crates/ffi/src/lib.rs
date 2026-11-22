//! iOS 原生壳的 C ABI：S3-only。
//!
//! 这里不启动本地 HTTP 服务，也不链接业务层的平台 provider。Swift 通过
//! tm_start 初始化，tm_invoke 阻塞执行一条命令，结果用 JSON 信封返回。
//!
//! ABI：
//!   const char *tm_start(const char *config_json);
//!   const char *tm_invoke(const char *cmd, const char *args_json);
//!   void tm_stop(void);
//!   void tm_free(char *ptr);
//!
//! 返回的字符串由 tm_free 释放。所有命令只在后台线程调用，不要在主线程阻塞。

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use tinymusic_core::favorites;
use tinymusic_core::http::HttpClient;
use tinymusic_core::library::{self, S3TrackRow};
use tinymusic_core::model::{Availability, Track};
use tinymusic_core::s3::{S3Config, S3};
use tinymusic_core::storage::Storage;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IosSettings {
    #[serde(default)]
    s3_enabled: bool,
    #[serde(default)]
    s3_endpoint: String,
    #[serde(default = "default_region")]
    s3_region: String,
    #[serde(default)]
    s3_bucket: String,
    #[serde(default = "default_prefix")]
    s3_prefix: String,
    #[serde(default)]
    s3_path_style: bool,
}

fn default_region() -> String {
    "us-east-1".into()
}

fn default_prefix() -> String {
    "TinyMusic".into()
}

impl Default for IosSettings {
    fn default() -> Self {
        Self {
            s3_enabled: false,
            s3_endpoint: String::new(),
            s3_region: default_region(),
            s3_bucket: String::new(),
            s3_prefix: default_prefix(),
            s3_path_style: false,
        }
    }
}

#[derive(Debug, Clone)]
struct Credentials {
    access_key: String,
    secret_key: String,
}

struct IosState {
    config_dir: PathBuf,
    http: Arc<HttpClient>,
    storage: Arc<Storage>,
    settings: Mutex<IosSettings>,
    credentials: Mutex<Option<Credentials>>,
    /// 同一个 runtime 上并发 block_on 容易踩调度/生命周期问题；v1 直接串行化命令。
    invoke_lock: Mutex<()>,
}

struct Running {
    runtime: Arc<tokio::runtime::Runtime>,
    state: Arc<IosState>,
}

static RUNNING: Mutex<Option<Running>> = Mutex::new(None);

fn settings_path(dir: &Path) -> PathBuf {
    dir.join("ios-settings.json")
}

fn load_settings(dir: &Path) -> IosSettings {
    std::fs::read_to_string(settings_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(dir: &Path, settings: &IosSettings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(dir), text).map_err(|e| format!("保存设置失败: {e}"))
}

impl IosState {
    fn new(config_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
        let settings = load_settings(&config_dir);
        let storage = Storage::open(config_dir.join("tinymusic.db"))
            .map_err(|e| format!("打开数据库失败: {e}"))?;
        Ok(Self {
            config_dir,
            http: Arc::new(HttpClient::new(0, 2)),
            storage: Arc::new(storage),
            settings: Mutex::new(settings),
            credentials: Mutex::new(None),
            invoke_lock: Mutex::new(()),
        })
    }

    fn settings_snapshot(&self) -> IosSettings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn has_credentials(&self) -> bool {
        self.credentials
            .lock()
            .map(|c| {
                c.as_ref()
                    .map(|c| !c.access_key.trim().is_empty() && !c.secret_key.trim().is_empty())
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    fn client(&self) -> Result<S3, String> {
        let s = self.settings_snapshot();
        let creds = self
            .credentials
            .lock()
            .map(|c| c.clone())
            .unwrap_or(None)
            .ok_or_else(|| "请先在设置里填写 S3 Access Key / Secret Key".to_string())?;
        let cfg = S3Config {
            endpoint: s.s3_endpoint.trim().to_string(),
            region: s.s3_region.trim().to_string(),
            bucket: s.s3_bucket.trim().to_string(),
            access_key: creds.access_key.trim().to_string(),
            secret_key: creds.secret_key.trim().to_string(),
            prefix: s.s3_prefix.clone(),
            path_style: s.s3_path_style,
        };
        if !cfg.is_ready() {
            return Err("请先填写 Endpoint / Bucket / Access Key / Secret Key".into());
        }
        Ok(S3::new(cfg, self.http.client().clone()))
    }

    fn set_settings(&self, settings: IosSettings) -> Result<(), String> {
        save_settings(&self.config_dir, &settings)?;
        if let Ok(mut slot) = self.settings.lock() {
            *slot = settings;
        }
        Ok(())
    }

    fn set_credentials(&self, access_key: String, secret_key: String) -> Result<(), String> {
        let access_key = access_key.trim().to_string();
        let secret_key = secret_key.trim().to_string();
        if access_key.is_empty() || secret_key.is_empty() {
            return Err("Access Key / Secret Key 不能为空".into());
        }
        if let Ok(mut slot) = self.credentials.lock() {
            *slot = Some(Credentials {
                access_key,
                secret_key,
            });
        }
        Ok(())
    }

    fn clear_credentials(&self) {
        if let Ok(mut slot) = self.credentials.lock() {
            *slot = None;
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn playable(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    [".mp3", ".m4a", ".aac", ".flac", ".wav"]
        .iter()
        .any(|e| lower.ends_with(e))
}

fn track_dto(row: &S3TrackRow) -> Value {
    let (quality, format) = library::s3_audio_format(&row.key);
    json!({
        "key": row.key,
        "title": row.track.title,
        "artist": row.track.artist,
        "cover": row.track.cover,
        "missing": row.missing,
        "codec": format.codec,
        "quality": quality.as_str(),
        "playable": playable(&row.key),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CredentialsArgs {
    access_key: String,
    secret_key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsArgs {
    settings: IosSettings,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistTracksArgs {
    name: String,
    #[serde(default)]
    root: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyArgs {
    key: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FavoriteAddArgs {
    key: String,
    title: String,
    artist: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderArgs {
    names: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NameArgs {
    name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RenameArgs {
    old: String,
    new: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveTrackArgs {
    key: String,
    target: String,
}

async fn dispatch(state: &IosState, cmd: &str, args: Value) -> Result<Value, String> {
    match cmd {
        "app_info" => Ok(json!({
            "version": env!("CARGO_PKG_VERSION"),
            "configDir": state.config_dir.to_string_lossy(),
        })),

        "settings_get" => {
            let s = state.settings_snapshot();
            let mut v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
            v["has_credentials"] = json!(state.has_credentials());
            Ok(v)
        }
        "settings_set" => {
            let a: SettingsArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            state.set_settings(a.settings)?;
            Ok(json!({ "ok": true }))
        }

        "s3_set_credentials" => {
            let a: CredentialsArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            state.set_credentials(a.access_key, a.secret_key)?;
            Ok(json!({ "ok": true }))
        }
        "s3_credentials_status" => Ok(json!({ "has_credentials": state.has_credentials() })),
        "s3_clear_credentials" => {
            state.clear_credentials();
            Ok(json!({ "ok": true }))
        }

        "s3_test" => {
            let client = state.client()?;
            let msg = client.test().await.map_err(|e| e.to_string())?;
            Ok(json!(msg))
        }

        "s3_playlists" => {
            let client = state.client()?;
            let list = library::list_s3_playlists(&client, &state.storage).await?;
            Ok(serde_json::to_value(list).map_err(|e| e.to_string())?)
        }

        "s3_playlist_tracks" => {
            let a: PlaylistTracksArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            let rows = library::s3_playlist_tracks(&client, &a.name, a.root).await?;
            Ok(Value::Array(rows.iter().map(track_dto).collect()))
        }

        "s3_play_url" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            let url = client
                .presign_get(&a.key, 6 * 3600)
                .map_err(|e| e.to_string())?;
            let (quality, format) = library::s3_audio_format(&a.key);
            Ok(json!({
                "url": url,
                "quality": quality.as_str(),
                "format": format,
                "expiresAt": now_secs() + 6 * 3600,
            }))
        }

        "s3_cover" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            match library::signed_cover_for(&client, &a.key).await {
                Some(url) => Ok(json!(url)),
                None => Ok(Value::Null),
            }
        }

        "s3_lyrics" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            let lrc_key = library::replace_ext(&a.key, "lrc");
            let bytes = client
                .get_object(&lrc_key)
                .await
                .map_err(|e| e.to_string())?;
            match bytes {
                Some(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let lines = library::parse_lrc(&text);
                    Ok(json!({ "present": true, "lines": lines }))
                }
                None => Ok(json!({ "present": false, "lines": [] })),
            }
        }

        "favorites_list" => {
            let list: Vec<Track> = state
                .storage
                .list_favorites()
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|t| t.provider == "s3")
                .collect();
            Ok(serde_json::to_value(list).map_err(|e| e.to_string())?)
        }

        "favorites_pull" => {
            let fallback = || -> Result<Value, String> {
                let list: Vec<Track> = state
                    .storage
                    .list_favorites()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .filter(|t| t.provider == "s3")
                    .collect();
                Ok(serde_json::to_value(list).map_err(|e| e.to_string())?)
            };
            match state.client() {
                Ok(client) => match favorites::pull(&client, &state.storage).await {
                    Ok(list) => Ok(serde_json::to_value(list).map_err(|e| e.to_string())?),
                    Err(_) => fallback(),
                },
                Err(_) => fallback(),
            }
        }

        "favorites_add" => {
            let a: FavoriteAddArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let track = Track {
                provider: "s3".into(),
                id: a.key,
                title: a.title,
                artist: a.artist,
                availability: Availability::Available,
                ..Track::default()
            };
            // 收藏 = 写网盘 favorites.json。桶里已经有这首歌就只写一条清单，
            // 「喜欢」目录只在网盘里没有这首歌时才存兜底副本（iOS 只收藏桶里的歌）。
            let client = state.client()?;
            let (synced, error, key) = match favorites::add(&client, &state.storage, &track).await {
                Ok(favorites::FavoriteAdd::Added { key }) => (true, Value::Null, json!(key)),
                Ok(favorites::FavoriteAdd::NeedsAudio { .. }) => (
                    false,
                    json!("这首歌还不在网盘里，先在桌面端下载到网盘再收藏"),
                    Value::Null,
                ),
                Err(e) => (false, json!(e), Value::Null),
            };
            Ok(json!({ "synced": synced, "error": error, "key": key }))
        }

        "favorites_remove" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            // 先写清单再删对象；只有指向「喜欢」兜底目录的副本才会删文件。
            let (synced, error) = match state.client() {
                Ok(client) => match favorites::remove(&client, &state.storage, &a.key).await {
                    Ok(()) => (true, Value::Null),
                    Err(e) => (false, json!(e)),
                },
                Err(e) => (false, json!(e)),
            };
            Ok(json!({ "synced": synced, "error": error }))
        }

        "favorites_check" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let yes = state
                .storage
                .is_favorite("s3", &a.key)
                .map_err(|e| e.to_string())?;
            Ok(json!(yes))
        }

        "playlists_reorder" => {
            let a: ReorderArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            state
                .storage
                .reorder_s3_playlists(&a.names)
                .map_err(|e| e.to_string())?;
            Ok(json!({ "ok": true }))
        }

        "playlist_create" => {
            let a: NameArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            let name = library::create_s3_playlist(&client, &state.storage, &a.name).await?;
            Ok(json!(name))
        }

        "playlist_rename" => {
            let a: RenameArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            let name = library::rename_s3_playlist(&client, &state.storage, &a.old, &a.new).await?;
            Ok(json!(name))
        }

        "playlist_delete" => {
            let a: NameArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            library::purge_s3_playlist(&client, &state.storage, &a.name).await?;
            Ok(json!({ "ok": true }))
        }

        "track_delete" => {
            let a: KeyArgs = serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            library::delete_s3_track(&client, &state.storage, &a.key).await?;
            Ok(json!({ "ok": true }))
        }

        "track_move" => {
            let a: MoveTrackArgs =
                serde_json::from_value(args).map_err(|e| format!("参数错误: {e}"))?;
            let client = state.client()?;
            library::move_s3_track(&client, &state.storage, &a.key, &a.target).await?;
            Ok(json!({ "ok": true }))
        }

        other => Err(format!("未知命令: {other}")),
    }
}

fn envelope(result: Result<Value, String>) -> String {
    match result {
        Ok(data) => serde_json::to_string(&json!({ "ok": true, "data": data }))
            .unwrap_or_else(|_| r#"{"ok":false,"error":"序列化失败"}"#.into()),
        Err(error) => serde_json::to_string(&json!({ "ok": false, "error": error }))
            .unwrap_or_else(|_| r#"{"ok":false,"error":"序列化失败"}"#.into()),
    }
}

fn into_c_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(c) => c.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

#[derive(Debug, Deserialize)]
struct StartConfig {
    config_dir: String,
}

/// 启动后端，返回 {"ok":true,"version":"..."} 或 {"ok":false,"error":"..."}
///
/// SAFETY: config_json 必须是合法的以 NUL 结尾的 UTF-8 C 字符串；返回值需要 tm_free 释放。
#[no_mangle]
pub unsafe extern "C" fn tm_start(config_json: *const c_char) -> *mut c_char {
    if config_json.is_null() {
        return into_c_string(envelope(Err("config_json 为空".into())));
    }
    let raw = match CStr::from_ptr(config_json).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return into_c_string(envelope(Err("config_json 不是 UTF-8".into()))),
    };
    let cfg: StartConfig = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => return into_c_string(envelope(Err(format!("解析配置失败: {e}")))),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => Arc::new(r),
        Err(e) => return into_c_string(envelope(Err(format!("创建 runtime 失败: {e}")))),
    };
    let state = match IosState::new(PathBuf::from(cfg.config_dir)) {
        Ok(s) => Arc::new(s),
        Err(e) => return into_c_string(envelope(Err(e))),
    };

    let config_dir = state.config_dir.to_string_lossy().to_string();
    if let Ok(mut slot) = RUNNING.lock() {
        *slot = Some(Running { runtime, state });
    }
    into_c_string(envelope(Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "configDir": config_dir,
    }))))
}

/// 执行一条命令。返回 {"ok":true,"data":...} 或 {"ok":false,"error":"..."}。
///
/// SAFETY: cmd / args_json 必须是合法的以 NUL 结尾的 UTF-8 C 字符串（args_json 可为空）；
/// 返回值需要 tm_free 释放。不要在 UI 主线程调用。
#[no_mangle]
pub unsafe extern "C" fn tm_invoke(cmd: *const c_char, args_json: *const c_char) -> *mut c_char {
    if cmd.is_null() {
        return into_c_string(envelope(Err("cmd 为空".into())));
    }
    let cmd = match CStr::from_ptr(cmd).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return into_c_string(envelope(Err("cmd 不是 UTF-8".into()))),
    };
    let args = if args_json.is_null() {
        json!({})
    } else {
        match CStr::from_ptr(args_json).to_str() {
            Ok(s) if !s.trim().is_empty() => match serde_json::from_str(s) {
                Ok(v) => v,
                Err(e) => return into_c_string(envelope(Err(format!("参数不是合法 JSON: {e}")))),
            },
            _ => json!({}),
        }
    };

    let (runtime, state) = match RUNNING.lock() {
        Ok(slot) => match slot.as_ref() {
            Some(r) => (r.runtime.clone(), r.state.clone()),
            None => return into_c_string(envelope(Err("后端尚未启动".into()))),
        },
        Err(_) => return into_c_string(envelope(Err("后端状态锁不可用".into()))),
    };

    let _guard = match state.invoke_lock.lock() {
        Ok(g) => g,
        Err(_) => return into_c_string(envelope(Err("命令锁不可用".into()))),
    };

    let result = catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(dispatch(&state, &cmd, args))
    }))
    .unwrap_or_else(|_| Err("后端命令异常退出".into()));

    into_c_string(envelope(result))
}

#[no_mangle]
pub extern "C" fn tm_stop() {
    if let Ok(mut slot) = RUNNING.lock() {
        *slot = None;
    }
}

/// 释放 tm_start / tm_invoke 返回的字符串。
///
/// SAFETY: 只能传上述函数返回的指针，且只能释放一次。
#[no_mangle]
pub unsafe extern "C" fn tm_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_state(name: &str) -> Arc<IosState> {
        let dir = std::env::temp_dir().join(format!("tm-ios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Arc::new(IosState::new(dir).expect("state"))
    }

    #[tokio::test]
    async fn settings_roundtrip_and_credentials_are_memory_only() {
        let st = temp_state("settings");
        let v = dispatch(&st, "settings_get", json!({})).await.unwrap();
        assert_eq!(v["s3Region"], "us-east-1");
        assert_eq!(v["has_credentials"], false);

        dispatch(
            &st,
            "s3_set_credentials",
            json!({ "accessKey": "AK", "secretKey": "SK" }),
        )
        .await
        .unwrap();
        let v = dispatch(&st, "settings_get", json!({})).await.unwrap();
        assert_eq!(v["has_credentials"], true);

        let settings_path = st.config_dir.join("ios-settings.json");
        let raw = std::fs::read_to_string(&settings_path).unwrap_or_default();
        assert!(!raw.contains("AK"), "settings.json 不应落盘 AK: {raw}");
        assert!(!raw.contains("SK"), "settings.json 不应落盘 SK: {raw}");
    }

    #[tokio::test]
    async fn missing_credentials_returns_actionable_error() {
        let st = temp_state("no-creds");
        let err = dispatch(&st, "s3_playlists", json!({})).await.unwrap_err();
        assert!(err.contains("Access Key"), "{err}");
    }

    #[tokio::test]
    async fn unknown_command_is_reported() {
        let st = temp_state("unknown");
        let err = dispatch(&st, "no_such_cmd", json!({})).await.unwrap_err();
        assert!(err.contains("未知命令"), "{err}");
    }

    #[tokio::test]
    async fn playable_formats_are_narrowed_for_avplayer() {
        assert!(playable("a.mp3"));
        assert!(playable("a.FLAC"));
        assert!(!playable("a.ogg"));
    }
}
