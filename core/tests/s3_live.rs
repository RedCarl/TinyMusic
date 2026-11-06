//! 网盘（S3）联通性/生命周期测试，默认忽略：
//!
//! ```sh
//! S3_EP=... S3_BUCKET=... S3_AK=... S3_SK=... \
//!   cargo test -p tinymusic-core --test s3_live -- --ignored --nocapture
//! ```

use tinymusic_core::s3::{S3Config, S3};

fn client() -> S3 {
    let cfg = S3Config {
        endpoint: std::env::var("S3_EP").expect("S3_EP"),
        region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        bucket: std::env::var("S3_BUCKET").expect("S3_BUCKET"),
        access_key: std::env::var("S3_AK").expect("S3_AK"),
        secret_key: std::env::var("S3_SK").expect("S3_SK"),
        prefix: "TinyMusic".into(),
        path_style: true,
    };
    S3::new(cfg, reqwest::Client::new())
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
    let key = std::env::var("S3_TEST_KEY")
        .unwrap_or_else(|_| "TinyMusic/喜欢/周杰伦 - 稻香.lrc".into());

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
