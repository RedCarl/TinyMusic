//! 网盘（S3）联通性/生命周期测试，默认忽略：
//!
//! ```sh
//! S3_EP=... S3_BUCKET=... S3_AK=... S3_SK=... \
//!   cargo test -p tinymusic-core --test s3_live -- --ignored --nocapture
//! ```

use tinymusic_core::s3::{S3Config, S3};

fn client_with_prefix(prefix: &str) -> S3 {
    let cfg = S3Config {
        endpoint: std::env::var("S3_EP").expect("S3_EP"),
        region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        bucket: std::env::var("S3_BUCKET").expect("S3_BUCKET"),
        access_key: std::env::var("S3_AK").expect("S3_AK"),
        secret_key: std::env::var("S3_SK").expect("S3_SK"),
        prefix: prefix.to_string(),
        path_style: true,
    };
    S3::new(cfg, reqwest::Client::new())
}

fn client() -> S3 {
    client_with_prefix("TinyMusic")
}

#[tokio::test]
#[ignore]
async fn s3_list_prefixes() {
    let s3 = client();
    match s3.test().await {
        Ok(msg) => println!("TEST OK: {}", msg),
        Err(e) => println!("TEST ERR: {}", e),
    }
    for p in ["TinyMusic", "TinyMusic/喜欢", "TinyMusic/favorites.json"] {
        match s3.list_keys(p).await {
            Ok(keys) => {
                println!("PREFIX {:?}: {} keys", p, keys.len());
                for k in keys.iter().take(10) {
                    println!("   - {}", k);
                }
            }
            Err(e) => println!("PREFIX {:?}: ERR {}", p, e),
        }
    }
}

#[tokio::test]
#[ignore]
async fn s3_presign_copy_delete() {
    let s3 = client();
    let key =
        std::env::var("S3_TEST_KEY").unwrap_or_else(|_| "TinyMusic/喜欢/周杰伦 - 稻香.lrc".into());

    let url = s3.presign_get(&key, 600).expect("presign");
    println!("PRESIGN: {}", url);
    let resp = reqwest::Client::new().get(&url).send().await.expect("GET");
    let status = resp.status();
    let body = resp.bytes().await.expect("body");
    println!("PRESIGN STATUS: {} bytes={}", status, body.len());
    assert!(status.is_success());

    let moved_key = "TinyMusic/__move_test__.lrc";
    s3.move_object(&key, moved_key).await.expect("move");
    let moved = s3.get_object(moved_key).await.expect("get moved");
    assert!(moved.is_some(), "移动后对象应存在");
    println!("MOVE OK bytes={}", moved.unwrap().len());

    s3.delete_object(moved_key).await.expect("delete");
    let gone = s3.get_object(moved_key).await.expect("get after delete");
    assert!(gone.is_none(), "删除后应为 None");
    println!("DELETE OK");
}
#[tokio::test]
#[ignore]
async fn s3_playlist_crud_lifecycle() {
    use tinymusic_core::library;
    use tinymusic_core::storage::Storage;

    let s3 = client();
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let source = format!("CRUD源{suffix}");
    let target = format!("CRUD目标{suffix}");
    let renamed = format!("CRUD改名{suffix}");
    let dir = std::env::temp_dir().join(format!("tinymusic-crud-{suffix}"));
    std::fs::create_dir_all(&dir).unwrap();
    let storage = Storage::open(dir.join("test.db")).expect("storage");

    let source_key = format!("TinyMusic/{source}/Artist - Song.mp3");
    let lrc_key = format!("TinyMusic/{source}/Artist - Song.lrc");
    s3.put_object(&source_key, b"audio".to_vec(), "audio/mpeg")
        .await
        .expect("put audio");
    s3.put_object(&lrc_key, b"[00:00.00]hi".to_vec(), "text/plain")
        .await
        .expect("put lrc");
    storage.save_s3_playlist(&source).unwrap();

    library::create_s3_playlist(&s3, &storage, &target)
        .await
        .expect("create target");
    library::move_s3_track(&s3, &storage, &source_key, &target)
        .await
        .expect("move track");
    assert!(s3.get_object(&source_key).await.unwrap().is_none());
    let moved_key = format!("TinyMusic/{target}/Artist - Song.mp3");
    assert!(s3.get_object(&moved_key).await.unwrap().is_some());
    let moved_lrc = format!("TinyMusic/{target}/Artist - Song.lrc");
    assert!(s3.get_object(&moved_lrc).await.unwrap().is_some());

    library::rename_s3_playlist(&s3, &storage, &target, &renamed)
        .await
        .expect("rename");
    let renamed_key = format!("TinyMusic/{renamed}/Artist - Song.mp3");
    assert!(s3.get_object(&renamed_key).await.unwrap().is_some());
    assert!(s3.get_object(&moved_key).await.unwrap().is_none());

    library::purge_s3_playlist(&s3, &storage, &renamed)
        .await
        .expect("purge");
    assert!(s3
        .list_keys(&format!("TinyMusic/{renamed}/"))
        .await
        .unwrap()
        .is_empty());
    assert!(!storage
        .list_s3_playlists()
        .unwrap()
        .iter()
        .any(|n| n == &renamed));

    let _ = std::fs::remove_dir_all(&dir);
}

/// 收藏本体是 favorites.json：「喜欢」目录只兜底存网盘里没有的歌。
///
/// 本地跑法（桩）：
///   cargo run -p tinymusic-server --example s3_stub -- --port 9107 --dir /tmp/tm-fav
///   S3_EP=http://127.0.0.1:9107 S3_BUCKET=music S3_AK=x S3_SK=y \
///     cargo test -p tinymusic-core --test s3_live favorites -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn s3_favorites_manifest_roundtrip() {
    use tinymusic_core::favorites;
    use tinymusic_core::model::Track;
    use tinymusic_core::storage::Storage;

    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // 独立前缀：不碰真实桶里的收藏，删也只删这一份
    let prefix = format!("TinyMusicTest{suffix}");
    let s3 = client_with_prefix(&prefix);
    let folder = tinymusic_core::library::favorites_folder(&prefix);
    let playlist = format!("Fav源{suffix}");
    let src_key = format!("{prefix}/{playlist}/Artist{suffix} - Song{suffix}.mp3");
    let src_lrc = format!("{prefix}/{playlist}/Artist{suffix} - Song{suffix}.lrc");
    let kept_key = format!("{folder}Kept{suffix} - Alone{suffix}.mp3");
    s3.put_object(&src_key, b"audio".to_vec(), "audio/mpeg")
        .await
        .expect("put audio");
    s3.put_object(&src_lrc, b"[00:00.00]hi".to_vec(), "text/plain")
        .await
        .expect("put lrc");
    s3.put_object(&kept_key, b"audio".to_vec(), "audio/mpeg")
        .await
        .expect("put kept");

    let dir = std::env::temp_dir().join(format!("tinymusic-fav-{suffix}"));
    std::fs::create_dir_all(&dir).unwrap();
    let storage = Storage::open(dir.join("test.db")).expect("storage");
    let fav = |id: &str, title: &str, artist: &str| Track {
        provider: "s3".into(),
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
        ..Track::default()
    };
    // 本地收藏：桶里有的、文件已经不在的、以及一条平台收藏
    storage
        .add_favorite(&fav(
            &src_key,
            &format!("Song{suffix}"),
            &format!("Artist{suffix}"),
        ))
        .unwrap();
    let gone_key = format!("{folder}Gone{suffix} - Vanished{suffix}.mp3");
    storage
        .add_favorite(&fav(
            &gone_key,
            &format!("Vanished{suffix}"),
            &format!("Gone{suffix}"),
        ))
        .unwrap();
    storage
        .add_favorite(&Track {
            provider: "netease".into(),
            id: "42".into(),
            title: "Remote".into(),
            artist: "平台".into(),
            ..Track::default()
        })
        .unwrap();

    // 1) 迁移：只写清单，不复制任何音频
    let list = favorites::pull(&s3, &storage).await.expect("pull");
    let copied_guess = format!("{folder}Artist{suffix} - Song{suffix}.mp3");
    assert!(
        s3.get_object(&copied_guess).await.unwrap().is_none(),
        "桶里已有的歌不该再复制一份到喜欢目录"
    );
    let ids: Vec<String> = list
        .iter()
        .filter(|t| t.provider == "s3")
        .map(|t| t.id.clone())
        .collect();
    assert!(ids.contains(&src_key), "喜欢要指向歌单里的原文件: {ids:?}");
    assert!(
        ids.contains(&kept_key),
        "兜底目录里已有的那首不能掉: {ids:?}"
    );
    assert!(!ids.contains(&gone_key), "文件没了就不该再算喜欢: {ids:?}");
    assert!(
        list.iter().any(|t| t.provider == "netease"),
        "还没同步的平台喜欢要留在列表里"
    );
    let manifest = favorites::load_manifest(&s3)
        .await
        .expect("read manifest")
        .expect("manifest 已落盘");
    assert_eq!(manifest.items.len(), 2, "清单里只有两首真在桶里的");
    assert_eq!(manifest.version, favorites::MANIFEST_VERSION);

    // 1.5) 喜欢一首「歌单里已经有」的歌：只写清单，不复制
    let src2_key = format!("{prefix}/{playlist}/Artist{suffix} - Another{suffix}.flac");
    s3.put_object(&src2_key, b"flac".to_vec(), "audio/flac")
        .await
        .expect("put another");
    let track = Track {
        provider: "s3".into(),
        id: src2_key.clone(),
        title: format!("Another{suffix}"),
        artist: format!("Artist{suffix}"),
        ..Track::default()
    };
    match favorites::add(&s3, &storage, &track).await.expect("add") {
        favorites::FavoriteAdd::Added { key } => assert_eq!(key, src2_key),
        favorites::FavoriteAdd::NeedsAudio { .. } => panic!("桶里已有的歌不该要求上传"),
    }
    assert!(
        s3.get_object(&format!("{folder}Artist{suffix} - Another{suffix}.flac"))
            .await
            .unwrap()
            .is_none(),
        "点喜欢不该把歌单里的歌再复制一份"
    );

    // 2) 取消喜欢：指向歌单的那条只删清单，文件不动
    favorites::remove(&s3, &storage, &src_key)
        .await
        .expect("unfavorite playlist");
    assert!(s3.get_object(&src_key).await.unwrap().is_some());
    assert!(s3.get_object(&src_lrc).await.unwrap().is_some());

    // 3) 取消喜欢：只有兜底副本的那条要删文件
    favorites::remove(&s3, &storage, &kept_key)
        .await
        .expect("unfavorite fallback");
    assert!(s3.get_object(&kept_key).await.unwrap().is_none());

    // 4) 去重维护：喜欢里多一份同歌副本 → 能扫出来、能只删那份
    let dup = format!("{folder}Artist{suffix} - Song{suffix}.mp3");
    s3.put_object(&dup, b"audio".to_vec(), "audio/mpeg")
        .await
        .expect("put dup");
    let report = favorites::dedupe_report(&s3).await.expect("dedupe report");
    assert!(
        report
            .iter()
            .any(|d| d.duplicate == dup && d.canonical == src_key),
        "应报出喜欢目录里的重复副本: {report:?}"
    );
    let freed = favorites::dedupe_purge(&s3, std::slice::from_ref(&dup))
        .await
        .expect("purge");
    assert!(freed > 0);
    assert!(s3.get_object(&dup).await.unwrap().is_none());
    assert!(s3.get_object(&src_key).await.unwrap().is_some());

    // 清理：整个测试前缀（含清单）一起删
    let _ = s3.delete_object(&favorites::manifest_key(&prefix)).await;
    if let Ok(keys) = s3.list_keys_all(&format!("{prefix}/")).await {
        for key in keys {
            let _ = s3.delete_object(&key).await;
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 超过阈值必须走分片：反向代理把 client_max_body_size 卡在 50MB 时，
/// 整包 PUT 会被 413 挡掉，30MB 的曲子必须还能传上去。
///
/// 真机跑法：
/// S3_EP=... S3_BUCKET=... S3_AK=... S3_SK=... \
///   cargo test -p tinymusic-core --test s3_live multipart -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn s3_multipart_upload_roundtrip() {
    let s3 = client();
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let key = format!("TinyMusic/__multipart_test_{suffix}.bin");

    // 默认 30MB ＞ 24MB 阈值：必定走分片；内容可校验，避免「传上去是空的」。
    // 想验更大的文件（比如 60MB，反向代理 50m 上限下整包必被 413）：
    //   S3_TEST_MB=60 cargo test -p tinymusic-core --test s3_live multipart -- --ignored
    let size_mb: usize = std::env::var("S3_TEST_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let size = size_mb * 1024 * 1024;
    let body: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    println!("UPLOAD {} bytes", body.len());
    s3.put_object(&key, body.clone(), "application/octet-stream")
        .await
        .expect("multipart put");

    let got = s3.get_object(&key).await.expect("get").expect("exists");
    println!("DOWNLOAD {} bytes", got.len());
    assert_eq!(got.len(), body.len(), "分片上传后大小应一致");
    assert!(got == body, "内容应逐字节一致");

    s3.delete_object(&key).await.expect("cleanup delete");
    println!("MULTIPART OK, cleanup done");
}
