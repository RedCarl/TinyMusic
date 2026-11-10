//! 本地流代理：`127.0.0.1:{port}/stream/{token}` → CDN。
//!
//! 为什么需要它：
//! * 把会过期的 CDN 直链藏在后端，前端只拿到短期 token；
//! * 透传 `Range`，让 `<audio>` 能拖进度条（实测 CDN 支持 206）；
//! * 规避 WebView 对第三方域的策略差异。
//!
//! 不引 hyper：手写最小 HTTP/1.1，只实现 GET/HEAD + Range，约 200 行。

use crate::error::Result;
use crate::http::HttpClient;
use crate::quality::normalize_content_type;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const TOKEN_TTL: std::time::Duration = std::time::Duration::from_secs(300);
const MAX_HEADER: usize = 8192;

struct Entry {
    url: String,
    /// 以解码后的实际格式为准；CDN 的 `application/octet-stream` 不能直接给 <audio>。
    content_type: Option<String>,
    expires: std::time::Instant,
}

pub struct StreamProxy {
    pub addr: SocketAddr,
    tokens: Arc<Mutex<HashMap<String, Entry>>>,
    counter: Arc<AtomicU64>,
}

impl StreamProxy {
    /// 启动代理，绑定 127.0.0.1:0
    pub async fn start(http: Arc<HttpClient>) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tokens: Arc<Mutex<HashMap<String, Entry>>> = Arc::new(Mutex::new(HashMap::new()));
        let counter = Arc::new(AtomicU64::new(1));

        let t = tokens.clone();
        tokio::spawn(async move {
            loop {
                let (sock, _) = match listener.accept().await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let t2 = t.clone();
                let h2 = http.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, t2, h2).await;
                });
            }
        });

        Ok(Self {
            addr,
            tokens,
            counter,
        })
    }

    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// 登记一个直链，返回可供前端使用的 URL。
    ///
    /// `content_type` 由调用方传入校验后的 MIME（例如 `audio/flac`）；
    /// 这比依赖 CDN 返回的头更可靠。
    pub async fn register(&self, url: String, content_type: Option<String>) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let token = format!("{:x}{:x}", now, n);
        let mut map = self.tokens.lock().await;
        // 顺手清理过期 token
        let now_i = std::time::Instant::now();
        map.retain(|_, e| e.expires > now_i);
        map.insert(
            token.clone(),
            Entry {
                url,
                content_type,
                expires: now_i + TOKEN_TTL,
            },
        );
        format!("{}/stream/{}", self.base_url(), token)
    }
}

async fn handle(
    mut sock: TcpStream,
    tokens: Arc<Mutex<HashMap<String, Entry>>>,
    http: Arc<HttpClient>,
) -> Result<()> {
    let mut buf = Vec::with_capacity(1024);
    let mut tmp = [0u8; 1024];
    // 读到请求头结束
    loop {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > MAX_HEADER {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let mut range: Option<String> = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.eq_ignore_ascii_case("range") {
                range = Some(v.trim().to_string());
            }
        }
    }

    if method == "OPTIONS" {
        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: *\r\nAccess-Control-Allow-Methods: GET,HEAD,OPTIONS\r\nConnection: close\r\n\r\n";
        sock.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let token = match path.strip_prefix("/stream/") {
        Some(t) => t.split('?').next().unwrap_or(""),
        None => {
            let _ = write_plain(&mut sock, 404, "not found").await;
            return Ok(());
        }
    };

    let (url, registered_type) = {
        let map = tokens.lock().await;
        match map.get(token) {
            Some(e) if e.expires > std::time::Instant::now() => {
                (e.url.clone(), e.content_type.clone())
            }
            _ => {
                let _ = write_plain(&mut sock, 404, "token expired").await;
                return Ok(());
            }
        }
    };

    // 上游请求（透传 Range）
    let upstream = match http.get_range(&url, range.as_deref()).await {
        Ok(r) => r,
        Err(_) => {
            let _ = write_plain(&mut sock, 502, "upstream error").await;
            return Ok(());
        }
    };

    let status = upstream.status();
    let upstream_ct = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/mpeg");
    let ct = registered_type
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| normalize_content_type(upstream_ct));
    let len = upstream
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let crange = upstream
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut head = String::new();
    head.push_str(if status == reqwest::StatusCode::PARTIAL_CONTENT {
        "HTTP/1.1 206 Partial Content\r\n"
    } else {
        "HTTP/1.1 200 OK\r\n"
    });
    head.push_str(&format!("Content-Type: {}\r\n", ct));
    head.push_str("Accept-Ranges: bytes\r\n");
    head.push_str("Access-Control-Allow-Origin: *\r\n");
    head.push_str("Cache-Control: no-store\r\n");
    if let Some(l) = &len {
        head.push_str(&format!("Content-Length: {}\r\n", l));
    }
    if let Some(cr) = &crange {
        head.push_str(&format!("Content-Range: {}\r\n", cr));
    }
    head.push_str("Connection: close\r\n\r\n");
    sock.write_all(head.as_bytes()).await?;

    if method != "HEAD" {
        let mut upstream = upstream;
        loop {
            match upstream.chunk().await {
                Ok(Some(chunk)) => {
                    if sock.write_all(&chunk).await.is_err() {
                        break; // 客户端断开（拖动进度条时会频繁发生），正常现象
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }
    let _ = sock.shutdown().await;
    Ok(())
}

async fn write_plain(sock: &mut TcpStream, code: u16, msg: &str) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        code,
        if code == 200 { "OK" } else { "Error" },
        msg.len(),
        msg
    );
    sock.write_all(resp.as_bytes()).await
}
