//! 联网集成测试（默认忽略，需手动执行）：
//!
//! ```sh
//! cargo test -p tinymusic-core --test live -- --ignored --nocapture
//! ```

use std::sync::Arc;
use tinymusic_core::download::{build_filename, DownloadExtras, Downloader};
use tinymusic_core::http::HttpClient;
use tinymusic_core::model::{MusicProvider, Quality, Track};
use tinymusic_core::providers::kuwo::KuwoProvider;

fn provider() -> KuwoProvider {
    KuwoProvider::with_http(Arc::new(HttpClient::new(700, 3)))
}

fn track(id: &str, duration_sec: u32) -> Track {
    Track {
        provider: "kuwo".into(),
        id: id.into(),
        duration_sec,
        ..Track::default()
    }
}

#[tokio::test]
#[ignore]
async fn live_search() {
    let p = provider();
    let page = p.search("稻香", 0, 10).await.expect("搜索失败");
    println!("total={} 返回={}", page.total, page.items.len());
    assert!(!page.items.is_empty());
    for t in page.items.iter().take(3) {
        println!(
            "  [{}] {} - {} / {} / {}s / cover={}",
            t.id,
            t.title,
            t.artist,
            t.album,
            t.duration_sec,
            t.cover.is_some()
        );
        assert!(!t.title.is_empty());
        assert!(!t.artist.is_empty());
        assert!(t.duration_sec > 0);
    }
}

#[tokio::test]
#[ignore]
async fn live_resolve_free_track() {
    let p = provider();
    // 192525785：实测可匿名获取整曲
    let info = p
        .resolve(&track("192525785", 0), Quality::Standard)
        .await
        .expect("应可解析");
    println!(
        "url={} bytes={:?} type={} availability={:?}",
        info.url, info.bytes, info.content_type, info.availability
    );
    assert!(info.url.starts_with("https://"));
    assert!(info.bytes.unwrap_or(0) > 500_000, "应拿到整曲而非片段");
}

#[tokio::test]
#[ignore]
async fn live_resolve_restricted_returns_unavailable() {
    // 注意：这个测试的语义在打通资源管理链路后变了。
    // 以前 440613（稻香原版，PAY=16711935）匿名只能拿到 11 秒占位文件；
    // 现在 provider 会落到 trackmedia 链路拿到真直链，所以「受限曲」不再是不可用。
    // 这里改为验证「不存在的曲目」仍然如实返回错误，避免静默成功。
    let p = provider();
    let r = p
        .resolve(&track("999999999999", 0), Quality::Standard)
        .await;
    match r {
        Err(tinymusic_core::Error::Unavailable)
        | Err(tinymusic_core::Error::NotFound(_))
        | Err(tinymusic_core::Error::Parse(_)) => {}
        other => panic!(
            "不存在的曲目应当报错，实际: {:?}",
            other.map(|i| (i.url, i.bytes))
        ),
    }
}

#[tokio::test]
#[ignore]
async fn live_download_writes_tagged_file() {
    let http = Arc::new(HttpClient::new(700, 3));
    let p = KuwoProvider::with_http(http.clone());
    let page = p.search("稻香", 0, 10).await.expect("搜索失败");
    // 找一首能取到整曲的
    let mut picked = None;
    for t in &page.items {
        if let Ok(info) = p.resolve(t, Quality::Standard).await {
            picked = Some((t.clone(), info));
            break;
        }
    }
    let (track, info) = picked.expect("十首里应当有可播放的");

    let dl = Downloader::new(http);
    let dir = std::env::temp_dir().join("tinymusic_live_dl");
    let fname = build_filename(&track, "mp3");
    // 顺带验证封面 + 歌词：真实运行时由外壳传入，这里用空 extras 保证链路可编译
    let extras = DownloadExtras::default();
    let path = dl
        .run(
            &track,
            &info,
            &dir,
            &fname,
            &extras,
            Box::new(|done, total| {
                if let Some(t) = total {
                    if done == t {
                        println!("  下载完成 {done} 字节");
                    }
                }
            }),
        )
        .await
        .expect("下载失败");

    let meta = std::fs::metadata(&path).unwrap();
    println!("  落盘: {:?} ({} 字节)", path, meta.len());
    assert!(meta.len() > 500_000);

    let head = std::fs::read(&path).unwrap();
    assert_eq!(&head[0..3], b"ID3", "应带 ID3 标签");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
#[ignore]
async fn live_artist_and_album_grouping() {
    let p = provider();
    let a = p.artist("336", "周杰伦", 0).await.expect("艺人页失败");
    println!(
        "艺人 {}: {} 首, {} 张专辑",
        a.name,
        a.tracks.len(),
        a.albums.len()
    );
    assert!(!a.tracks.is_empty());
    assert!(!a.albums.is_empty());
    for al in a.albums.iter().take(3) {
        println!("  专辑 [{}] {}", al.id, al.name);
    }

    let alb = a.albums.first().unwrap().clone();
    let page = p.album(&alb.id, &alb.name, 0).await.expect("专辑页失败");
    println!("专辑 {}: {} 首", page.name, page.tracks.len());
    assert!(!page.tracks.is_empty());
}

#[tokio::test]
#[ignore]
async fn live_stream_proxy_supports_range() {
    use tinymusic_core::proxy::StreamProxy;

    let http = Arc::new(HttpClient::new(700, 3));
    let p = KuwoProvider::with_http(http.clone());
    let info = p
        .resolve(&track("192525785", 0), Quality::Standard)
        .await
        .expect("取链失败");

    let proxy = StreamProxy::start(http).await.expect("代理启动失败");
    println!("代理地址: {}", proxy.base_url());
    let local = proxy
        .register(info.url.clone(), Some(info.content_type.clone()))
        .await;
    println!("本地地址: {}", local);

    // 用原始 TCP 发一个带 Range 的请求，避免引入额外 HTTP 客户端
    let addr = proxy.addr;
    let path = local.split(&proxy.base_url()).nth(1).unwrap().to_string();

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nRange: bytes=0-1023\r\n\r\n",
        path
    );
    stream.write_all(req.as_bytes()).await.unwrap();

    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        let n = stream.read(&mut tmp).await.unwrap();
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 8192 {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    println!(
        "响应头:\n{}",
        head.lines().take(8).collect::<Vec<_>>().join("\n")
    );
    assert!(
        head.starts_with("HTTP/1.1 206"),
        "应返回 206 Partial Content"
    );
    assert!(head.to_lowercase().contains("content-range"));
    assert!(head.contains("1024"), "Content-Length 应为 1024");

    // 不存在的 token
    let mut s2 = tokio::net::TcpStream::connect(addr).await.unwrap();
    s2.write_all(b"GET /stream/nope HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut b2 = Vec::new();
    let _ = s2.read_to_end(&mut b2).await;
    let h2 = String::from_utf8_lossy(&b2);
    assert!(h2.starts_with("HTTP/1.1 404"), "未知 token 应 404");
}

#[tokio::test]
#[ignore]
async fn live_playlist_search_and_open() {
    let p = provider();
    let page = p
        .search_playlists("周杰伦", 0, 5)
        .await
        .expect("歌单搜索失败");
    println!("歌单总数={} 返回={}", page.total, page.items.len());
    assert!(!page.items.is_empty());
    for x in page.items.iter().take(3) {
        println!(
            "  [{}] {} / {} / {} 首 / cover={}",
            x.id,
            x.name,
            x.creator.as_deref().unwrap_or("-"),
            x.track_count,
            x.cover.is_some()
        );
        assert!(!x.id.is_empty());
        assert!(!x.name.is_empty());
        if let Some(c) = &x.cover {
            assert!(!c.contains("kwcdn"), "封面域名未归一化: {}", c);
        }
    }

    // 打开第一个歌单，验证分页拉取与曲目映射
    let pid = page.items[0].id.clone();
    let d = p.playlist(&pid).await.expect("歌单详情失败");
    println!(
        "歌单《{}》 标称 {} 首, 实际拉取 {} 首",
        d.playlist.name,
        d.playlist.track_count,
        d.tracks.len()
    );
    assert!(!d.tracks.is_empty());
    let t = &d.tracks[0];
    println!("  首曲: {} - {} ({}s)", t.title, t.artist, t.duration_sec);
    assert!(!t.title.is_empty());
    assert!(t.duration_sec > 0);

    // 歌单里的曲目必须能走通取链（不一定成功，但不能 panic）
    let r = p.resolve(t, Quality::Standard).await;
    println!(
        "  取链结果: {}",
        if r.is_ok() {
            "可播"
        } else {
            "不可用（受限曲，符合预期）"
        }
    );
}

#[tokio::test]
#[ignore]
async fn live_auth_capabilities_are_declared() {
    let p = provider();
    let caps = p.auth_capabilities();
    println!("酷我登录能力:");
    for c in &caps {
        println!(
            "  {:?} supported={} reason={}",
            c.method,
            c.supported,
            c.reason.as_deref().unwrap_or("-")
        );
    }
    // 声明不支持的必须带原因
    for c in &caps {
        if !c.supported {
            assert!(c.reason.is_some(), "{:?} 缺原因", c.method);
        }
    }
    // my_playlists 应给出可读的 Unsupported，而不是静默空列表
    let sess = tinymusic_core::model::Session::default();
    match p.my_playlists(&sess).await {
        Err(tinymusic_core::Error::Unsupported(_)) => {}
        other => panic!("期望 Unsupported，实际 {:?}", other.map(|v| v.len())),
    }
}

/// 付费曲（稻香 440613，PAY=16711935）在**未登录**状态下应能取到完整音频。
///
/// 这是整条逆向链路的验收测试：anti.s 匿名只给 11 秒占位文件，
/// provider 应当自动落到资源管理链路拿到真直链。
#[tokio::test]
#[ignore]
async fn live_paid_song_plays_without_login() {
    let p = provider();
    for q in [Quality::Lossless, Quality::Super, Quality::Standard] {
        let info = p.resolve(&track("440613", 223), q).await.expect("取链失败");
        let bytes = info.bytes.unwrap_or(0);
        println!(
            "  {:?} -> {} bytes ({:.1} MB), ctype={}, avail={:?}, actual={:?}, format={:?}, degraded={}",
            q,
            bytes,
            bytes as f64 / 1_048_576.0,
            info.content_type,
            info.availability,
            info.quality,
            info.format,
            info.degraded
        );
        assert_eq!(
            info.availability,
            tinymusic_core::model::Availability::Available
        );
        assert!(
            bytes > 1_000_000,
            "{:?} 拿到的是 {} 字节，疑似占位文件",
            q,
            bytes
        );
        assert!(!info.url.contains("588957081"), "{:?} 命中占位 URL", q);
        assert_eq!(info.quality, q, "{:?} 的实际档位应一致", q);
        assert_eq!(
            info.format.lossless,
            q == Quality::Lossless,
            "{:?} 的无损标记不对",
            q
        );
    }
}

/// 按 uid 列歌单：酷我这条链路不需要登录
#[tokio::test]
#[ignore]
async fn live_user_playlists_by_uid() {
    let p = provider();
    let list = p.user_playlists("182253281", 50).await.expect("取歌单失败");
    println!("uid=182253281 歌单数: {}", list.len());
    assert!(!list.is_empty(), "应当能列出歌单");
    for x in list.iter().take(5) {
        println!("  [{}] {} / {} 首", x.id, x.name, x.track_count);
        assert!(!x.id.is_empty());
        assert!(!x.name.is_empty());
    }
    // 私密歌单也应能打开
    let d = p.playlist(&list[0].id).await.expect("打开歌单失败");
    println!("  打开《{}》拿到 {} 首", d.playlist.name, d.tracks.len());
    assert!(!d.tracks.is_empty() || list[0].track_count == 0);
}

/// 搜索页要用：搜到歌之后能拿到该艺人的专辑列表
#[tokio::test]
#[ignore]
async fn live_search_artist_has_albums() {
    let p = provider();
    let page = p.search("稻香", 0, 20).await.expect("搜索失败");
    let first = page
        .items
        .iter()
        .find(|t| !t.artist_id.is_empty())
        .expect("搜索结果里应有带 artist_id 的曲目");
    println!(
        "首个结果: {} - {} (artist_id={})",
        first.title, first.artist, first.artist_id
    );
    let ap = p
        .artist(&first.artist_id, &first.artist, 0)
        .await
        .expect("艺人页失败");
    println!(
        "艺人 {} 专辑数 {} / 曲目 {}",
        ap.name,
        ap.albums.len(),
        ap.tracks.len()
    );
    for a in ap.albums.iter().take(5) {
        println!("  [{}] {} ({:?})", a.id, a.name, a.year);
    }
    assert!(!ap.albums.is_empty(), "艺人应有专辑");
}

/// 列表没带 MINFO 时，musicinfo 探测应给出可播档位。
#[tokio::test]
#[ignore]
async fn live_track_qualities() {
    let p = provider();
    let t = track("440613", 223);
    let q = p.track_qualities(&t).await.expect("音质探测失败");
    println!(
        "available={:?} has_advanced={} known={}",
        q.available, q.has_advanced, q.known
    );
    assert!(q.known);
    assert!(q.available.contains(&Quality::Standard));
    assert!(q.available.contains(&Quality::Super));
    assert!(q.available.contains(&Quality::Lossless));
    assert!(q.has_advanced, "该曲目录里有加密高级格式");
}

/// 高级档位必须明确拒绝，不能返回占位文件冒充。
#[tokio::test]
#[ignore]
async fn live_advanced_quality_is_rejected() {
    let p = provider();
    let t = track("440613", 223);
    for q in [Quality::HiRes, Quality::Atmos, Quality::Master] {
        let r = p.resolve(&t, q).await;
        println!("{:?} -> {:?}", q, r.as_ref().map(|i| i.quality).err());
        assert!(r.is_err(), "{:?} 不应返回可播 URL", q);
    }
}

/// 没有 ALFLAC 的曲目请求无损时，应回退到 128/320 并标记 degraded，
/// 不能把 128k MP3 标成“无损”。
#[tokio::test]
#[ignore]
async fn live_lossless_falls_back_honestly() {
    let p = provider();
    // 200677874：实测无 ALFLAC（48s 短曲）
    let t = track("200677874", 48);
    let info = p
        .resolve(&t, Quality::Lossless)
        .await
        .expect("应有可播档位");
    println!(
        "requested={:?} actual={:?} degraded={} format={:?} bytes={:?}",
        info.requested, info.quality, info.degraded, info.format, info.bytes
    );
    assert_ne!(info.quality, Quality::Lossless, "没有 FLAC 不能标无损");
    assert!(info.degraded);
}

/// 无损下载必须保留 fLaC 文件头，不能把 ID3 塞到前面。
#[tokio::test]
#[ignore]
async fn live_download_lossless_keeps_flac_header() {
    let http = Arc::new(HttpClient::new(700, 3));
    let p = KuwoProvider::with_http(http.clone());
    let t = track("440613", 223);
    let info = p.resolve(&t, Quality::Lossless).await.expect("取无损失败");
    assert_eq!(info.quality, Quality::Lossless);

    let dl = Downloader::new(http);
    let dir = std::env::temp_dir().join("tinymusic_live_flac");
    let path = dl
        .run(
            &t,
            &info,
            &dir,
            &build_filename(&t, "flac"),
            &DownloadExtras::default(),
            Box::new(|_, _| {}),
        )
        .await
        .expect("无损下载失败");
    let head = std::fs::read(&path).unwrap();
    println!(
        "  落盘 {} 字节, head={:?}",
        head.len(),
        &head[..4.min(head.len())]
    );
    assert_eq!(&head[..4], b"fLaC", "FLAC 文件头被破坏");
    let _ = std::fs::remove_dir_all(&dir);
}
