//! Tauri 桌面外壳：窗口、托盘、更新、登录 WebView 等桌面专属能力。
//!
//! 业务逻辑不在这里：桌面与 mobile 共用 `tinymusic_app`（同一份命令实现与 dispatch）。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager, State};

use tinymusic_app::{
    AppState, Host, Progress, ProgressFn, S3FixFn, S3FixProgress, S3ImportResult, S3IntegrityReport,
};
use tinymusic_core::model::*;
use tinymusic_core::storage::FailedDownload;


/// 网易云登录窗口的 label（与酷我登录分开）
const NETEASE_LOGIN_LABEL: &str = "nelogin";

/// 把后端事件转发到前端：桌面用 Tauri 事件总线（前端 `listen` 收到）
struct AppHost {
    app: tauri::AppHandle,
}

impl Host for AppHost {
    fn failed(&self, failures: &[FailedDownload]) {
        let _ = self.app.emit("download://failed", failures);
    }
}

/// 通用命令入口：前端用 `invoke('tm_invoke', { cmd, args })` 调，落到 `tinymusic_app::dispatch`
#[tauri::command]
async fn tm_invoke(
    state: State<'_, Arc<AppState>>,
    cmd: String,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    tinymusic_app::dispatch::dispatch(state.inner(), &cmd, args, None, None).await
}

/// 以下 6 个命令带进度通道（Tauri Channel 只能在类型化命令里传），
/// 逻辑仍然只有 tinymusic_app 里的那一份，这里只是把 Channel 包成 ProgressFn。
#[tauri::command]
async fn download_start(
    state: State<'_, Arc<AppState>>,
    track: Track,
    quality: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<String, String> {
    let sink: ProgressFn = Arc::new(move |p: Progress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::download_start(state.inner(), track, quality, sink).await
}

#[tauri::command]
async fn download_batch(
    state: State<'_, Arc<AppState>>,
    tracks: Vec<Track>,
    quality: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<Vec<String>, String> {
    let sink: ProgressFn = Arc::new(move |p: Progress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::download_batch(state.inner(), tracks, quality, sink).await
}

#[tauri::command]
async fn s3_download(
    state: State<'_, Arc<AppState>>,
    track: Track,
    quality: Option<String>,
    playlist: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<String, String> {
    let sink: ProgressFn = Arc::new(move |p: Progress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::s3_download(state.inner(), track, quality, playlist, sink).await
}

#[tauri::command]
async fn s3_download_batch(
    state: State<'_, Arc<AppState>>,
    tracks: Vec<Track>,
    quality: Option<String>,
    playlist: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<Vec<String>, String> {
    let sink: ProgressFn = Arc::new(move |p: Progress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::s3_download_batch(state.inner(), tracks, quality, playlist, sink).await
}

#[tauri::command]
async fn s3_integrity_fix(
    state: State<'_, Arc<AppState>>,
    playlist: Option<String>,
    ids: Vec<String>,
    on_progress: Channel<S3FixProgress>,
) -> Result<S3IntegrityReport, String> {
    let sink: S3FixFn = Arc::new(move |p: S3FixProgress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::s3_integrity_fix(state.inner(), playlist, ids, sink).await
}

#[tauri::command]
async fn s3_import_playlist(
    state: State<'_, Arc<AppState>>,
    provider: String,
    playlist_id: String,
    name: String,
    target: Option<String>,
    on_progress: Channel<Progress>,
) -> Result<S3ImportResult, String> {
    let sink: ProgressFn = Arc::new(move |p: Progress| {
        let _ = on_progress.send(p);
    });
    tinymusic_app::s3_import_playlist(state.inner(), provider, playlist_id, name, target, sink).await
}

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
    // 网页里的 QQ/微信扫码是 window.open 出来的，webview 不一定能触发新窗口请求，
    // 这里直接接管，交给主进程开窗口。
    // 微信图标被点击时，不管站点自己的 window.open 是否被拦，都直接通知主进程开扫码窗
    if (!window.__kwWxBound) {
      window.__kwWxBound = true;
      document.addEventListener('click', function (e) {
        try {
          var el = e.target;
          for (var i = 0; i < 4 && el; i++) {
            var tx = (el.textContent || '').trim();
            if (tx === '微信' || (tx.indexOf('微信') >= 0 && tx.length < 12)) {
              e.preventDefault();
              e.stopPropagation();
              document.cookie = 'kwprobe_wx=1;path=/;max-age=60';
              return;
            }
            el = el.parentElement;
          }
        } catch (err) {}
      }, true);
    }

    if (!window.__kwOpenPatched) {
      window.__kwOpenPatched = true;
      window.open = function (u) {
        try {
          // 远程页面没有 IPC 权限，用 cookie 把扫码地址交给主进程
          document.cookie = 'kwprobe_pop=' + encodeURIComponent(String(u)) + ';path=/;max-age=300';
        } catch (e) {}
        return null;
      };
    }

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
            // QQ / 微信登录会 window.open 一个扫码页；WKWebView/WebView2 默认弹不出来，这里显式建一个
            static POPUP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = POPUP_SEQ.fetch_add(1, Ordering::Relaxed);
            let label = format!("kwlogin-pop-{seq}");
            let target = url.clone();
            eprintln!("[tinymusic] login popup -> {}", target);
            let builder = tauri::WebviewWindowBuilder::new(
                &popup_app,
                &label,
                tauri::WebviewUrl::External(target.clone()),
            )
            // 共享父 webview 的配置/环境，opener 关系和 cookie 才能对上
            .window_features(features)
            .inner_size(430.0, 640.0)
            .center()
            .always_on_top(true)
            .title("微信登录")
            .on_document_title_changed(|window, title| {
                let _ = window.set_title(&title);
            });
            match builder.build() {
                Ok(window) => {
                    let _ = window.set_focus();
                    tauri::webview::NewWindowResponse::Create { window }
                }
                Err(e) => {
                    eprintln!("[tinymusic] login popup failed: {e}");
                    // 兜底：建不出新窗口就直接在当前登录窗口里打开扫码页
                    if let Some(w) = popup_app.get_webview_window(LOGIN_LABEL) {
                        let _ = w.navigate(target);
                    }
                    tauri::webview::NewWindowResponse::Deny
                }
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
        for i in 0..900 {
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
            if i < 100 || i % 5 == 0 {
                let _ = w.eval(AUTO_LOGIN_JS);
            }
            let cookies = w.cookies().unwrap_or_default();
            let mut modal_open = false;
            let mut has_uid = false;
            let mut popup_url: Option<String> = None;
            let mut wechat_clicked = false;
            for c in &cookies {
                match c.name() {
                    "kwprobe_wx" => wechat_clicked = c.value() == "1",
                    "kwprobe_pop" => {
                        let v = url_decode(c.value());
                        if v.starts_with("http") {
                            popup_url = Some(v);
                        }
                    }
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

            if wechat_clicked {
                let _ = w.eval("document.cookie='kwprobe_wx=;path=/;max-age=0'");
                if let Err(e) = open_login_popup(&handle, &wechat_login_url()) {
                    eprintln!("[tinymusic] wechat popup failed: {e}");
                }
            }

            if let Some(url) = popup_url {
                let _ = w.eval("document.cookie='kwprobe_pop=;path=/;max-age=0'");
                if let Err(e) = open_login_popup(&handle, &url) {
                    eprintln!("[tinymusic] popup open failed: {e}");
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
            let mut b =
                tauri::webview::cookie::Cookie::build((c.name().to_string(), String::new()))
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
            let url = tauri::Url::parse("https://www.kuwo.cn/").map_err(|e| e.to_string())?;
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

    let _ =
        w.eval("try{localStorage.clear();sessionStorage.clear();document.cookie='';}catch(e){}");
    clear_kuwo_cookies(&w);

    if created {
        let _ = w.close();
    }
    Ok(())
}


/// 让窗口标题栏跟随应用主题；auto 时传 None，交还给系统
static LOGIN_POPUP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);


/// 开一个独立的扫码窗口（微信 / QQ 的 OAuth 页）
fn open_login_popup(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    let seq = LOGIN_POPUP_SEQ.fetch_add(1, Ordering::Relaxed);
    let label = format!("kwlogin-pop-{seq}");
    let target = tauri::Url::parse(url).map_err(|e| e.to_string())?;
    let w = tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::External(target))
        .inner_size(430.0, 640.0)
        .center()
        .always_on_top(true)
        .title("微信登录")
        .on_document_title_changed(|window, title| {
            let _ = window.set_title(&title);
        })
        .build()
        .map_err(|e| e.to_string())?;
    let _ = w.set_focus();
    Ok(())
}


fn url_encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}


/// 酷我微信扫码登录地址（与官网 wxLogin 的拼法一致）
fn wechat_login_url() -> String {
    let callback = url_encode_component("https://www.kuwo.cn/callback");
    let inner =
        format!("http://i.kuwo.cn/US/platform/WeixinCallback.jsp?f=web&callbackUrl={callback}");
    format!(
        "https://open.weixin.qq.com/connect/qrconnect?appid=wx41c1275bb3e28427&response_type=code&scope=snsapi_login&redirect_uri={}",
        url_encode_component(&inner)
    )
}


fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}


/// 由页面里的 window.open 钩子调用：开一个独立的扫码窗口
#[tauri::command]
async fn login_popup(app: tauri::AppHandle, url: String) -> Result<(), String> {
    open_login_popup(&app, &url)
}


#[tauri::command]
fn set_window_theme(window: tauri::WebviewWindow, theme: Option<String>) -> Result<(), String> {
    let t = match theme.as_deref() {
        Some("light") => Some(tauri::Theme::Light),
        Some("dark") => Some(tauri::Theme::Dark),
        _ => None,
    };
    window.set_theme(t).map_err(|e| e.to_string())
}


/// 系统版本：玻璃材质要按系统降级（macOS 11/12 的 WKWebView 太旧，
/// 模糊与 oklch 都靠不住，整页会用「边框材质」）。
///
/// 只在桌面壳里实现：前端拿不到这条命令时，只有能力检测兜底。
#[derive(serde::Serialize)]
struct SystemInfo {
    os: String,
    /// macOS 是 "12.7.6" 这种；Windows/Linux 留空（WebView2 是常青的，不用按系统降级）
    #[serde(rename = "osVersion")]
    os_version: String,
}

#[tauri::command]
fn system_info() -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        os_version: os_version(),
    }
}

#[cfg(target_os = "macos")]
fn os_version() -> String {
    use std::sync::OnceLock;
    // 启动后只问一次系统；sw_vers 是一次几十毫秒的子进程，别每帧都问
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        })
        .clone()
}

#[cfg(not(target_os = "macos"))]
fn os_version() -> String {
    String::new()
}

#[tauri::command]
fn login_close(app: tauri::AppHandle) -> Result<(), String> {
    close_login_popups(&app);
    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        w.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}


/// 打开网易云网页登录窗口：用户在里面正常登录，我们从 webview 里取 `MUSIC_U`。
///
/// 为什么不用「手机号 + 密码」当主路径：网易云对异地/新设备会要求短信或设备校验
/// （`code=8821`），纯 HTTP 登录很难过。网页登录走的是官方前端，风控由官方处理，
/// 我们只负责把 cookie 交回后端校验一次。
#[tauri::command]
async fn netease_login_open(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(NETEASE_LOGIN_LABEL) {
        let _ = w.set_focus();
        return Ok(());
    }
    let url = tauri::Url::parse("https://music.163.com/").map_err(|e| e.to_string())?;
    let w = tauri::WebviewWindowBuilder::new(
        &app,
        NETEASE_LOGIN_LABEL,
        tauri::WebviewUrl::External(url),
    )
    .title("网易云登录")
    .always_on_top(true)
    .inner_size(1100.0, 780.0)
    .visible(false)
    .on_document_title_changed(|window, title| {
        let _ = window.set_title(&title);
    })
    .build()
    .map_err(|e| e.to_string())?;
    place_centered(&w, 1100.0, 780.0);
    let _ = w.show();
    let _ = w.set_focus();

    // 轮询 cookie：拿到 MUSIC_U → 校验通过才关窗 + 落库 + 通知前端。
    //
    // 顺序很重要：以前是「先关窗再校验」。上一条 MUSIC_U 过期时关窗发生在校验之前，
    // 用户会看到窗口一闪就没、按钮一直转「等待登录…」，而弹窗已经没了、根本没法重新登录。
    // 现在校验失败就把窗口留着（并提一次错），等新的 MUSIC_U 出现再校验。
    let handle = app.clone();
    std::thread::spawn(move || {
        let mut misses = 0u8;
        // 已校验失败的 MUSIC_U：值没变就不再重复请求（否则每 500ms 打一次网易云）
        let mut rejected: Option<String> = None;
        for _ in 0..720 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let Some(w) = handle.get_webview_window(NETEASE_LOGIN_LABEL) else {
                misses += 1;
                if misses > 4 {
                    return;
                }
                continue;
            };
            misses = 0;
            let Ok(cookies) = w.cookies() else { continue };
            let picked: Vec<(String, String)> = cookies
                .iter()
                .filter(|c| {
                    let d = c.domain().unwrap_or("").to_ascii_lowercase();
                    d.contains("163.com")
                })
                .map(|c| (c.name().to_string(), c.value().to_string()))
                .collect();
            let Some(music_u) = picked
                .iter()
                .find(|(k, v)| k == "MUSIC_U" && !v.is_empty())
                .map(|(_, v)| v.clone())
            else {
                continue;
            };
            if rejected.as_deref() == Some(music_u.as_str()) {
                continue;
            }
            let header = picked
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("; ");

            // manage() 注册的是 Arc<AppState>（见 run() 里的 app.manage(state)）。
            // 这里若按 AppState 取会直接 panic —— release 是 panic=abort，表现就是点了登录立刻闪退。
            let state = handle.state::<Arc<AppState>>();
            let provider = state.provider("netease");
            let result = match provider {
                Ok(p) => tauri::async_runtime::block_on(p.login_cookie(&header)),
                Err(e) => Err(tinymusic_core::Error::Unsupported(e)),
            };
            match result {
                Ok(session) => {
                    let _ = state.storage.save_session(&session);
                    if let Ok(p) = state.provider("netease") {
                        p.set_session(&session);
                    }
                    let _ = w.close();
                    let _ = handle.emit("netease://login", &session);
                    return;
                }
                Err(e) => {
                    // 过期 / 无效的 MUSIC_U：窗口留着，让用户在里面重新登录
                    rejected = Some(music_u);
                    let _ = w.show();
                    let _ = w.set_focus();
                    let _ = handle.emit("netease://login_error", e.to_string());
                }
            }
        }
        // 超时：把窗口留着让用户自己关，但告知前端
        let _ = handle.emit("netease://login_timeout", "登录窗口已超时");
    });

    Ok(())
}


#[tauri::command]
fn open_download_dir(state: State<'_, Arc<AppState>>) -> Result<(), String> {
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


/// 根目录散歌在网盘页里的伪歌单名（不是真实目录，保留名不可用来建歌单）



fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}


/// 托盘菜单文案：跟随界面语言（前端启动和切换语言时都会调 set_ui_language）
fn tray_menu_for(app: &tauri::AppHandle, lang: &str) -> tauri::Result<Menu<tauri::Wry>> {
    let (show_t, toggle_t, prev_t, next_t, quit_t) = if lang == "en" {
        (
            "Show TinyMusic",
            "Play / Pause",
            "Previous",
            "Next",
            "Quit TinyMusic",
        )
    } else {
        ("显示主界面", "播放 / 暂停", "上一首", "下一首", "退出 TinyMusic")
    };
    let show = MenuItem::with_id(app, "show", show_t, true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", toggle_t, true, None::<&str>)?;
    let prev_i = MenuItem::with_id(app, "prev", prev_t, true, None::<&str>)?;
    let next_i = MenuItem::with_id(app, "next", next_t, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", quit_t, true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[&show, &sep1, &toggle, &prev_i, &next_i, &sep2, &quit],
    )
}

fn build_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let menu = tray_menu_for(app.handle(), "zh")?;

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


/// 界面语言变化时重画托盘菜单（前端 initI18n / setLanguage 会调用）
#[tauri::command]
fn set_ui_language(app: tauri::AppHandle, lang: String) -> Result<(), String> {
    let Some(tray) = app.tray_by_id("main") else {
        return Ok(());
    };
    let menu = tray_menu_for(&app, &lang).map_err(|e| e.to_string())?;
    tray.set_menu(Some(menu)).map_err(|e| e.to_string())
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
            let host = Arc::new(AppHost {
                app: app.handle().clone(),
            });
            let state = tauri::async_runtime::block_on(tinymusic_app::AppState::new(
                config_dir,
                std::path::PathBuf::from(tinymusic_app::dirs_download()),
                host,
            ))
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!("[tinymusic] 流代理: {}", state.proxy.base_url());
            app.manage(state);

            // 托盘图标 + 右键菜单：关窗口后还能控制播放
            build_tray(app)?;
            Ok(())
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            // 只有主窗口「关闭」才收进托盘；登录窗口该关就关
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<AppState>>();
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
            tm_invoke,
            download_start,
            download_batch,
            s3_download,
            s3_download_batch,
            s3_integrity_fix,
            s3_import_playlist,
            login_open,
            login_visible,
            login_probe,
            login_purge,
            login_popup,
            login_close,
            netease_login_open,
            open_download_dir,
            set_window_theme,
            set_ui_language,
            system_info,
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
