//! 反向代理把请求体卡得比 16MB 还小时（openresty 常见 1m / 8m / 10m），
//! 上传要自己缩分片，而不是把 413 甩给用户去重试五次。
//!
//! 这里起一个进程内的 S3 桩：超过上限直接回 413，其余按 S3 多段上传语义处理。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use tinymusic_core::s3::{S3Config, S3};

#[derive(Default)]
struct Stub {
    /// 单次请求体上限（字节）
    limit: usize,
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    uploads: Mutex<BTreeMap<String, BTreeMap<u32, Vec<u8>>>>,
    next_id: Mutex<u32>,
}

type Shared = Arc<Stub>;

const PAYLOAD_TOO_LARGE: &str =
    "<html><head><title>413 Request Entity Too Large</title></head></html>";

async fn put_object(
    State(stub): State<Shared>,
    Path((bucket, key)): Path<(String, String)>,
    Query(q): Query<BTreeMap<String, String>>,
    body: Bytes,
) -> Response {
    if body.len() > stub.limit {
        return (StatusCode::PAYLOAD_TOO_LARGE, PAYLOAD_TOO_LARGE).into_response();
    }
    let name = format!("{}/{}", bucket, key);
    if let (Some(part), Some(upload)) = (q.get("partNumber"), q.get("uploadId")) {
        let number: u32 = part.parse().unwrap_or(0);
        let mut uploads = stub.uploads.lock().unwrap();
        let Some(parts) = uploads.get_mut(upload) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        parts.insert(number, body.to_vec());
        let mut res = StatusCode::OK.into_response();
        res.headers_mut().insert(
            axum::http::header::ETAG,
            format!("\"part-{}\"", number).parse().unwrap(),
        );
        return res;
    }
    stub.objects.lock().unwrap().insert(name, body.to_vec());
    StatusCode::OK.into_response()
}

async fn post_object(
    State(stub): State<Shared>,
    Path((bucket, key)): Path<(String, String)>,
    Query(q): Query<BTreeMap<String, String>>,
    body: Bytes,
) -> Response {
    if body.len() > stub.limit {
        return (StatusCode::PAYLOAD_TOO_LARGE, PAYLOAD_TOO_LARGE).into_response();
    }
    if q.contains_key("uploads") {
        let mut next = stub.next_id.lock().unwrap();
        *next += 1;
        let id = format!("upload-{}", *next);
        stub.uploads
            .lock()
            .unwrap()
            .insert(id.clone(), BTreeMap::new());
        let xml = format!(
            "<InitiateMultipartUploadResult><UploadId>{}</UploadId></InitiateMultipartUploadResult>",
            id
        );
        return (StatusCode::OK, xml).into_response();
    }
    let Some(id) = q.get("uploadId") else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !String::from_utf8_lossy(&body).contains("<CompleteMultipartUpload>") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let parts = stub.uploads.lock().unwrap().remove(id);
    let Some(parts) = parts else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut merged = Vec::new();
    for (_, part) in parts {
        merged.extend_from_slice(&part);
    }
    stub.objects
        .lock()
        .unwrap()
        .insert(format!("{}/{}", bucket, key), merged);
    (StatusCode::OK, "<CompleteMultipartUploadResult/>").into_response()
}

async fn delete_object(
    State(stub): State<Shared>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    if let Some(id) = q.get("uploadId") {
        stub.uploads.lock().unwrap().remove(id);
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn get_object(
    State(stub): State<Shared>,
    Path((bucket, key)): Path<(String, String)>,
) -> Response {
    let name = format!("{}/{}", bucket, key);
    match stub.objects.lock().unwrap().get(&name) {
        Some(bytes) => (StatusCode::OK, bytes.clone()).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// 起一个桩，返回它的 endpoint
async fn spawn(limit: usize) -> String {
    let stub: Shared = Arc::new(Stub {
        limit,
        ..Default::default()
    });
    let app = Router::new()
        .route("/{bucket}", get(|| async { StatusCode::OK }))
        .route(
            "/{bucket}/{*key}",
            get(get_object)
                .put(put_object)
                .post(post_object)
                .delete(delete_object),
        )
        // 桩自己判 413，别让 axum 在读完 body 前就掐断连接
        .layer(DefaultBodyLimit::max(512 * 1024 * 1024))
        .with_state(stub);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{}", addr)
}

fn client(endpoint: String) -> S3 {
    S3::new(
        S3Config {
            endpoint,
            region: "us-east-1".into(),
            bucket: "music".into(),
            access_key: "ak".into(),
            secret_key: "sk".into(),
            prefix: "TinyMusic".into(),
            path_style: true,
        },
        reqwest::Client::new(),
    )
}

fn sample(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

/// 代理上限 6MB：16MB / 8MB 分片都会被 413 挡掉，缩到 5MB 才通。
#[tokio::test]
async fn shrinks_parts_when_proxy_limit_is_below_16mb() {
    const MB: usize = 1024 * 1024;
    let s3 = client(spawn(6 * MB).await);
    let body = sample(30 * MB);
    s3.put_object("TinyMusic/big.flac", body.clone(), "audio/flac")
        .await
        .expect("缩分片后应当上传成功");
    let got = s3
        .get_object("TinyMusic/big.flac")
        .await
        .expect("get")
        .expect("对象应当存在");
    assert_eq!(got.len(), body.len(), "分片拼回来的长度要一致");
    assert!(got == body, "内容要逐字节一致");
}

/// 整包 PUT 被挡（对象没到 24MB 阈值）时，也要自动改走分片。
#[tokio::test]
async fn falls_back_to_multipart_when_single_put_is_rejected() {
    const MB: usize = 1024 * 1024;
    let s3 = client(spawn(7 * MB).await);
    let body = sample(20 * MB);
    s3.put_object("TinyMusic/mid.flac", body.clone(), "audio/flac")
        .await
        .expect("整包被 413 后应当改走 5MB 分片");
    let got = s3
        .get_object("TinyMusic/mid.flac")
        .await
        .expect("get")
        .expect("对象应当存在");
    assert!(got == body);
}

/// 上限比 5MB 分片下限还小：分片救不了，报错必须说清楚该改哪里。
#[tokio::test]
async fn reports_actionable_error_when_limit_is_below_5mb() {
    const MB: usize = 1024 * 1024;
    let s3 = client(spawn(4 * MB).await);
    let err = s3
        .put_object("TinyMusic/big.flac", sample(30 * MB), "audio/flac")
        .await
        .expect_err("4MB 上限下 5MB 分片也过不去")
        .to_string();
    assert!(err.contains("413"), "要保留 HTTP 状态：{}", err);
    assert!(
        err.contains("client_max_body_size"),
        "要给出可操作的修改项：{}",
        err
    );
    assert!(err.contains("5.0MB"), "要说清分片下限：{}", err);
    assert!(err.contains("分片 "), "要说清是哪一片被挡的：{}", err);
}
