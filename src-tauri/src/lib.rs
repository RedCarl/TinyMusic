//! Tauri 外壳：状态装配 + IPC 命令面。业务逻辑全部在 `tinymusic-core`。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{Manager, State};

use tinymusic_core::download::{build_filename, Downloader};
use tinymusic_core::http::HttpClient;
use tinymusic_core::model::*;
use tinymusic_core::proxy::StreamProxy;
use tinymusic_core::providers::kuwo::KuwoProvider;
use tinymusic_core::storage::{DownloadRow, Storage};

// ---------- 设置 ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub download_dir: String,
    pub default_quality: String,
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
}

fn default_concurrency() -> u32 {
    2
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: dirs_download(),
            default_quality: "high".into(),
            concurrency: 2,
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
    let p = state.provider(&provider)?;
    p.detail(&track).await.map_err(|e| e.to_string())
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
    let track2 = track.clone();
    let quality2 = info.quality;

    tauri::async_runtime::spawn(async move {
        let ch = on_progress.clone();
        let tid2 = tid.clone();
        let res = dl
            .run(
                &track2,
                &info,
                &dir,
                &fname,
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
fn favorite_add(state: State<'_, AppState>, track: Track) -> Result<(), String> {
    state.storage.add_favorite(&track).map_err(|e| e.to_string())
}

#[tauri::command]
fn favorite_remove(
    state: State<'_, AppState>,
    provider: String,
    id: String,
) -> Result<(), String> {
    state
        .storage
        .remove_favorite(&provider, &id)
        .map_err(|e| e.to_string())
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
    // 1) 点开登录弹窗（只点一次）
    if (!window.__kwLoginOpened) {
      // 窄视口下站点会用媒体查询把「登录/注册」隐藏起来换成汉堡菜单。
      // 这里把它强制显示回来，这样窄窗口里也能直接点到。
      if (!window.__kwStyleInjected) {
        var st = document.createElement('style');
        st.textContent = '.reg_text{display:flex!important;visibility:visible!important;' +
                         'opacity:1!important;width:auto!important;height:auto!important;}' +
                         '.reg_text span{display:inline!important;}';
        document.head.appendChild(st);
        window.__kwStyleInjected = true;
      }
      var trigger = document.querySelector('.reg_text');
      if (trigger) {
        ['mousedown', 'mouseup', 'click'].forEach(function (type) {
          trigger.dispatchEvent(new MouseEvent(type, {
            bubbles: true, cancelable: true, view: window
          }));
        });
        window.__kwLoginOpened = true;
      }
    }
    // 2) 上报弹窗是否还开着：靠「固定定位 + 高 z-index + 体积够大」识别遮罩层。
    //    站点标题栏虽然也是 fixed，但高度很小，被宽高条件排除。
    var open = false;
    var nodes = document.querySelectorAll('div');
    for (var i = 0; i < nodes.length; i++) {
      var cs = getComputedStyle(nodes[i]);
      if (cs.position !== 'fixed' || cs.display === 'none' || cs.visibility === 'hidden') continue;
      var z = parseInt(cs.zIndex, 10) || 0;
      if (z < 100) continue;
      var r = nodes[i].getBoundingClientRect();
      if (r.width > 300 && r.height > 300) { open = true; break; }
    }
    document.cookie = 'kwprobe_modal=' + (open ? '1' : '0') + '; path=/; domain=.kuwo.cn';
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
    tauri::WebviewWindowBuilder::new(&app, LOGIN_LABEL, tauri::WebviewUrl::External(url))
        .title("酷我登录")
        // 普通窗口：保留标题栏和关闭按钮
        .always_on_top(true)
        .resizable(false)
        // 先按桌面宽度创建（此时窗口是隐藏的，用户看不到），
        // 否则窄视口下站点会切移动布局，「登录/注册」入口根本不存在，弹窗点不出来

        // 一开始就是弹窗尺寸；隐藏到登录框就绪再 show，避免白屏闪烁
        .visible(false)
        .inner_size(1000.0, 760.0)
        .build()
        .map_err(|e| e.to_string())?;


    // 后台协程：把弹窗点出来，并在网页弹窗被关掉时把这个无边框窗口一起关掉
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut saw_open = false;
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
                saw_open = true;
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
                return; // 登录成功，交给前端读取 uid
            }
            // 8 秒还没等到弹窗（站点改版等）：直接按原样显示，让用户自己点登录
            if !saw_open && i >= 40 && !isolated {
                let _ = w.set_resizable(true);
                let _ = w.set_size(tauri::LogicalSize::new(LOGIN_W, LOGIN_H));
                let _ = w.set_resizable(false);
                place_centered(&w, LOGIN_W, LOGIN_H);
                let _ = w.show();
                let _ = w.set_focus();
                saw_open = true;
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

#[tauri::command]
fn login_close(app: tauri::AppHandle) -> Result<(), String> {
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

        let ch = on_progress.clone();
        let tid = task_id.clone();
        let res = state
            .downloader
            .run(
                track,
                &info,
                &dir,
                &fname,
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
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
            favorite_add,
            favorite_remove,
            favorite_list,
            favorite_check,
            settings_get,
            settings_set,
            open_download_dir,
            search_playlists,
            user_playlists,
            login_open,
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
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}
