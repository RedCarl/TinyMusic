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

#[derive(Clone)]
enum Source {
    /// 远端直链（CDN / 网盘签名地址）
    Url(String),
    /// 已经下载到本地的文件
    File(std::path::PathBuf),
}

struct Entry {
    source: Source,
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
                source: Source::Url(url),
                content_type,
                expires: now_i + TOKEN_TTL,
            },
        );
        format!("{}/stream/{}", self.base_url(), token)
    }

    /// 登记一个本地文件，返回可供前端使用的 URL（离线播放用）。
    ///
    /// 走同一个代理，前端只接触 `127.0.0.1`，不用给 WebView 开 `file://` 权限。
    pub async fn register_file(
        &self,
        path: impl Into<std::path::PathBuf>,
        content_type: Option<String>,
    ) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let token = format!("{:x}{:x}", now, n);
        let mut map = self.tokens.lock().await;
        let now_i = std::time::Instant::now();
        map.retain(|_, e| e.expires > now_i);
        map.insert(
            token.clone(),
            Entry {
                source: Source::File(path.into()),
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

    let (source, registered_type) = {
        let map = tokens.lock().await;
        match map.get(token) {
            Some(e) if e.expires > std::time::Instant::now() => (e.source.clone(), e.content_type.clone()),
            _ => {
                let _ = write_plain(&mut sock, 404, "token expired").await;
                return Ok(());
            }
        }
    };

    if let Source::File(path) = &source {
        return serve_file(&mut sock, path, range.as_deref(), registered_type.as_deref(), method)
            .await;
    }
    let url = match source {
        Source::Url(u) => u,
        Source::File(_) => unreachable!(),
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

/// 把本地文件当流媒体发出去：支持 HEAD 与 `Range: bytes=a-b`。
async fn serve_file(
    sock: &mut TcpStream,
    path: &std::path::Path,
    range: Option<&str>,
    content_type: Option<&str>,
    method: &str,
) -> Result<()> {
    use tokio::io::{AsyncSeekExt, AsyncReadExt};

    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(_) => {
            let _ = write_plain(sock, 404, "file missing").await;
            return Ok(());
        }
    };
    let size = meta.len();
    let ct = content_type
        .map(|s| s.to_string())
        .unwrap_or_else(|| mime_for_path(path).to_string());

    // `bytes=start-` / `bytes=start-end`；其它写法一律当整段
    let mut start = 0u64;
    let mut end = size.saturating_sub(1);
    let mut partial = false;
    if let Some(raw) = range {
        if let Some(spec) = raw.trim().strip_prefix("bytes=") {
            let spec = spec.split(',').next().unwrap_or("").trim();
            let (a, b) = spec.split_once('-').unwrap_or((spec, ""));
            let a = a.trim();
            let b = b.trim();
            if !a.is_empty() {
                if let Ok(v) = a.parse::<u64>() {
                    start = v;
                }
            }
            if !b.is_empty() {
                if let Ok(v) = b.parse::<u64>() {
                    end = v.min(size.saturating_sub(1));
                }
            }
            if size == 0 || start >= size {
                let resp = format!(
                    "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{}\r\nConnection: close\r\n\r\n",
                    size
                );
                sock.write_all(resp.as_bytes()).await?;
                return Ok(());
            }
            partial = true;
        }
    }
    if end < start {
        end = start;
    }
    let len = end - start + 1;

    let mut head = String::new();
    head.push_str(if partial {
        "HTTP/1.1 206 Partial Content\r\n"
    } else {
        "HTTP/1.1 200 OK\r\n"
    });
    head.push_str(&format!("Content-Type: {}\r\n", ct));
    head.push_str("Accept-Ranges: bytes\r\n");
    head.push_str("Access-Control-Allow-Origin: *\r\n");
    head.push_str("Cache-Control: no-store\r\n");
    head.push_str(&format!("Content-Length: {}\r\n", len));
    if partial {
        head.push_str(&format!("Content-Range: bytes {}-{}/{}\r\n", start, end, size));
    }
    head.push_str("Connection: close\r\n\r\n");
    sock.write_all(head.as_bytes()).await?;

    if method == "HEAD" {
        let _ = sock.shutdown().await;
        return Ok(());
    }

    let mut file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    if start > 0 {
        let _ = file.seek(std::io::SeekFrom::Start(start)).await;
    }
    let mut left = len;
    let mut buf = vec![0u8; 64 * 1024];
    while left > 0 {
        let want = (buf.len() as u64).min(left) as usize;
        match file.read(&mut buf[..want]).await {
            Ok(0) => break,
            Ok(n) => {
                if sock.write_all(&buf[..n]).await.is_err() {
                    break; // 拖动进度条时客户端会主动断开
                }
                left -= n as u64;
            }
            Err(_) => break,
        }
    }
    let _ = sock.shutdown().await;
    Ok(())
}

/// 按扩展名给本地文件一个 WebView 认得出来的 MIME。
pub fn mime_for_path(path: &std::path::Path) -> &'static str {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".flac") {
        "audio/flac"
    } else if lower.ends_with(".mp3") {
        "audio/mpeg"
    } else if lower.ends_with(".m4a") || lower.ends_with(".mp4") || lower.ends_with(".aac") {
        "audio/mp4"
    } else if lower.ends_with(".ogg") {
        "audio/ogg"
    } else if lower.ends_with(".wav") {
        "audio/wav"
    } else {
        "application/octet-stream"
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Arc<HttpClient> {
        Arc::new(HttpClient::new(0, 4))
    }

    /// 本地文件（离线播放）要能整段读、也要能按 Range 读
    #[tokio::test]
    async fn serves_local_file_with_range() {
        let proxy = StreamProxy::start(client()).await.expect("proxy 应该能启动");
        let path = std::env::temp_dir().join(format!("tinymusic-proxy-test-{}.mp3", std::process::id()));
        let bytes: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
        tokio::fs::write(&path, &bytes).await.expect("写测试文件");

        let url = proxy
            .register_file(path.clone(), Some("audio/mpeg".into()))
            .await;

        let http = reqwest::Client::new();
        let full = http.get(&url).send().await.expect("整段请求");
        assert_eq!(full.status(), 200);
        assert_eq!(full.headers()["content-type"], "audio/mpeg");
        assert_eq!(full.headers()["accept-ranges"], "bytes");
        let body = full.bytes().await.expect("读整段");
        assert_eq!(body.len(), bytes.len());
        assert_eq!(&body[..], &bytes[..]);

        let part = http
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=1000-1099")
            .send()
            .await
            .expect("Range 请求");
        assert_eq!(part.status(), 206);
        assert_eq!(part.headers()["content-range"], "bytes 1000-1099/4096");
        let piece = part.bytes().await.expect("读片段");
        assert_eq!(piece.len(), 100);
        assert_eq!(&piece[..], &bytes[1000..1100]);

        let _ = tokio::fs::remove_file(&path).await;
    }

    /// 文件被删掉时明确返回 404，而不是挂住
    #[tokio::test]
    async fn missing_file_is_404() {
        let proxy = StreamProxy::start(client()).await.expect("proxy 应该能启动");
        let url = proxy
            .register_file(std::env::temp_dir().join("tinymusic-not-here.mp3"), None)
            .await;
        let resp = reqwest::Client::new().get(&url).send().await.expect("请求");
        assert_eq!(resp.status(), 404);
    }
}
