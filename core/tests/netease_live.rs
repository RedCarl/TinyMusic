//! 网易云链路的**真机验证**（默认 `#[ignore]`，需要外网）。
//!
//! ```bash
//! cargo test -p tinymusic-core --test netease_live -- --ignored --nocapture
//! # 带登录态（可选）：先设置环境变量，测「每日推荐」与会员曲
//! $env:NETEASE_COOKIE="MUSIC_U=..."; cargo test ... -- --ignored --nocapture
//! ```
//!
//! 三组验证，对应方案里的三个疑问：
//!
//! 1. `weapi` 加密是否被服务端接受（不是 400/加密错误）；
//! 2. 匿名/登录后，目录接口（歌单、推荐、搜索）能拿到什么；
//! 3. **核心问题**：网易云拿目录、酷我拿音源，这条路走得通吗？成功率多少？

use std::sync::Arc;
use tinymusic_core::model::{Availability, MusicProvider, Quality};
use tinymusic_core::providers::netease::NeteaseProvider;
use tinymusic_core::providers::kuwo::KuwoProvider;
use tinymusic_core::providers::netease::bridge;

fn providers() -> (Arc<dyn MusicProvider>, Arc<NeteaseProvider>) {
    let kuwo: Arc<dyn MusicProvider> = Arc::new(KuwoProvider::new());
    let ne = Arc::new(NeteaseProvider::new(Some(kuwo.clone())));
    (kuwo, ne)
}

async fn login_if_possible(ne: &NeteaseProvider) -> bool {
    let Ok(cookie) = std::env::var("NETEASE_COOKIE") else {
        return false;
    };
    if cookie.trim().is_empty() {
        return false;
    }
    match ne.login_cookie(&cookie).await {
        Ok(s) => {
            println!("    ✅ cookie 登录成功：{}（uid={}）", s.nickname, s.user_id);
            true
        }
        Err(e) => {
            println!("    ⚠️ cookie 登录失败：{e}");
            false
        }
    }
}

#[tokio::test]
#[ignore]
async fn weapi_is_accepted_and_catalog_works() {
    let (_kuwo, ne) = providers();
    println!("\n=== 1. weapi 加密是否被接受 ===");

    // 断言：匿名也能读的热歌榜
    let pl = ne.playlist("3778678").await.expect("歌单详情失败");
    println!(
        "    热歌榜：{} / {} 首（解析出 {} 首曲目）",
        pl.playlist.name,
        pl.playlist.track_count,
        pl.tracks.len()
    );
    assert!(pl.tracks.len() > 10, "只解析出 {} 首", pl.tracks.len());
    assert_eq!(pl.playlist.provider, "netease");

    let first = &pl.tracks[0];
    println!(
        "    第一条：{} - {} ({}s, {:?})",
        first.title, first.artist, first.duration_sec, first.availability
    );
    assert!(!first.title.is_empty());

    // 搜索
    let sp = ne.search("晴天", 0, 5).await.expect("搜索失败");
    println!("    搜索「晴天」：total={} 首条={}", sp.total, sp.items[0].title);
    assert!(!sp.items.is_empty());

    // 歌单搜索
    let pls = ne.search_playlists("华语", 0, 5).await.expect("歌单搜索失败");
    println!("    歌单搜索「华语」：total={}", pls.total);

    // 专辑 + 艺人 + 歌词
    let al = ne.album("32311", "神的游戏", 0).await.expect("专辑失败");
    println!("    专辑：{} / {} 首", al.name, al.tracks.len());
    let ar = ne.artist("6452", "周杰伦", 0).await.expect("艺人失败");
    println!(
        "    艺人：{} 热门 {} 首 / 专辑 {} 张",
        ar.name,
        ar.tracks.len(),
        ar.albums.len()
    );
    let d = ne.detail(&pl.tracks[0]).await.expect("歌词失败");
    println!("    歌词：{} 行", d.lyrics.len());
}

#[tokio::test]
#[ignore]
async fn catalog_gate_requires_login() {
    let (_kuwo, ne) = providers();
    println!("\n=== 2. 需要登录的目录接口 ===");
    let logged = login_if_possible(&ne).await;
    println!("    登录态：{}", if logged { "有" } else { "无" });

    match ne.recommend_songs(30).await {
        Ok(v) => println!("    每日推荐：{} 首，首条={}", v.len(), v.first().map(|t| t.title.as_str()).unwrap_or("-")),
        Err(e) => println!("    每日推荐：失败 → {e}"),
    }
    match ne.recommend_playlists().await {
        Ok(v) => println!("    推荐歌单：{} 张，首张={}", v.len(), v.first().map(|p| p.name.as_str()).unwrap_or("-")),
        Err(e) => println!("    推荐歌单：失败 → {e}"),
    }
    if logged {
        match ne.my_playlists(&Default::default()).await {
            Ok(v) => println!("    我的歌单：{} 张", v.len()),
            Err(e) => println!("    我的歌单：失败 → {e}"),
        }
    }
}

/// 核心验证：把歌单里的曲目逐首走 `resolve`，统计「网易云直链 / 酷我桥接」各自命中多少。
#[tokio::test]
#[ignore]
async fn playback_success_rate() {
    let (kuwo, ne) = providers();
    println!("\n=== 3. 播放成功率（网易云直链 + 酷我桥接） ===");
    let logged = login_if_possible(&ne).await;

    let pl = ne.playlist("3778678").await.expect("歌单详情失败");
    let sample: Vec<_> = pl.tracks.iter().take(20).cloned().collect();
    println!(
        "    样本：热歌榜前 {} 首｜登录态：{}\n",
        sample.len(),
        if logged { "有" } else { "无" }
    );

    let mut ne_direct = 0;
    let mut kuwo_bridge = 0;
    let mut failed = 0;

    for t in &sample {
        let want_kuwo = t.availability == Availability::VipOnly && !logged;
        let mut line = format!("    {:<24}", format!("{} - {}", trunc(&t.title, 14), trunc(&t.artist, 10)));

        // 先单看网易云直链
        let direct = ne.resolve_direct(t, Quality::Super).await;
        let direct_ok = direct.is_ok();
        if direct_ok {
            ne_direct += 1;
            let info = direct.unwrap();
            line.push_str(&format!(
                "网易云 ✅ {}k {}KB via={}",
                info.format.bitrate_kbps.unwrap_or(0),
                info.bytes.unwrap_or(0) / 1024,
                info.via
            ));
        } else {
            // 酷我桥接
            let m = bridge::find_in_kuwo(kuwo.as_ref(), t).await;
            match m {
                Some(m) => {
                    let played = kuwo.resolve(&m.track, Quality::Super).await;
                    match played {
                        Ok(info) => {
                            kuwo_bridge += 1;
                            line.push_str(&format!(
                                "酷我 ✅「{}」score={:.2} {}k {}KB",
                                trunc(&m.track.title, 12),
                                m.score,
                                info.format.bitrate_kbps.unwrap_or(0),
                                info.bytes.unwrap_or(0) / 1024
                            ));
                        }
                        Err(e) => {
                            failed += 1;
                            line.push_str(&format!(
                                "酷我 ⚠️ 命中但取流失败({e}) score={:.2}",
                                m.score
                            ));
                        }
                    }
                }
                None => {
                    failed += 1;
                    line.push_str("❌ 两边都没有");
                }
            }
            if want_kuwo {
                line.push_str("（会员曲，需登录网易云）");
            }
        }
        println!("{line}");
    }

    let total = sample.len();
    println!(
        "\n    结果：网易云直链 {}/{}｜酷我桥接 {}/{}｜失败 {}｜合计可播 {}/{}（{}%）",
        ne_direct,
        total,
        kuwo_bridge,
        total,
        failed,
        ne_direct + kuwo_bridge,
        total,
        (ne_direct + kuwo_bridge) * 100 / total.max(1)
    );
}

/// 匹配质量：拿「网易云自己取不到」的那批曲目（fee=1）来测桥接。
///
/// 用真实曲目（真实时长）而不是手写样本：时长是硬性否决条件，
/// 手填一个偏差 26 秒的时长会让好匹配直接判 0 分，测出来的东西没有意义。
#[tokio::test]
#[ignore]
async fn bridge_matching_quality() {
    let (kuwo, ne) = providers();
    println!("\n=== 4. 桥接匹配质量（酷我搜索 + 打分） ===");
    let pl = ne.playlist("3778678").await.expect("歌单详情失败");
    let sample: Vec<_> = pl
        .tracks
        .iter()
        .filter(|t| t.availability == Availability::VipOnly)
        .take(10)
        .cloned()
        .collect();
    if sample.is_empty() {
        println!("    今天的热歌榜没有 fee=1 曲目，跳过");
        return;
    }
    let mut hit = 0;
    for want in &sample {
        match bridge::find_in_kuwo(kuwo.as_ref(), want).await {
            Some(m) => {
                hit += 1;
                println!(
                    "    ✅ {} - {} ({}s) → 「{}」-「{}」({}s) score={:.2}（标题{:.2}/歌手{:.2}/时长{:.2}）",
                    want.title,
                    want.artist,
                    want.duration_sec,
                    m.track.title,
                    m.track.artist,
                    m.track.duration_sec,
                    m.score,
                    m.parts.0,
                    m.parts.1,
                    m.parts.2
                );
            }
            None => println!(
                "    ⬜ {} - {} ({}s) → 低于阈值，判为不可用",
                want.title, want.artist, want.duration_sec
            ),
        }
    }
    println!("\n    fee=1 样本命中 {}/{}", hit, sample.len());
}

fn trunc(s: &str, n: usize) -> String {
    let v: String = s.chars().take(n).collect();
    if s.chars().count() > n {
        format!("{v}…")
    } else {
        v
    }
}
