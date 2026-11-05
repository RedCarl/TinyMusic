//! 统一 HTTP 层：UA、按主机限速、退避重试、并发闸门。
//!
//! 实测约束：
//! * `m.kuwo.cn` 的详情接口会返回 `{"data":null,"msg":"音乐查询失败","status":301}`，
//!   属于软限流，必须按失败处理并退避。
//! * CDN 直链不需要任何特殊 header，支持 `Range`。

use crate::error::{backoff, Error, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};

pub const UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

/// 软限流的响应特征
const RATE_LIMIT_MARKERS: [&str; 3] = ["音乐查询失败", "\"status\":301", "\"status\": 301"];

pub struct HttpClient {
    client: reqwest::Client,
    /// 每个主机最近一次请求的时间戳（毫秒）
    last_hit: Arc<Mutex<HashMap<String, u64>>>,
    min_interval_ms: u64,
    permits: Arc<Semaphore>,
    pub stats: Arc<Stats>,
}

#[derive(Default)]
pub struct Stats {
    pub requests: AtomicU64,
    pub retries: AtomicU64,
    pub throttled: AtomicU64,
}

impl HttpClient {
    pub fn new(min_interval_ms: u64, max_concurrency: usize) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .pool_max_idle_per_host(8)
            .build()
            .expect("构建 HTTP 客户端失败");
        Self {
            client,
            last_hit: Arc::new(Mutex::new(HashMap::new())),
            min_interval_ms,
            permits: Arc::new(Semaphore::new(max_concurrency)),
            stats: Arc::new(Stats::default()),
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// 同一主机的最小请求间隔控制
    async fn throttle(&self, host_key: &str) {
        let wait = {
            let mut map = self.last_hit.lock().await;
            let now = Self::now_ms();
            let last = map.get(host_key).copied().unwrap_or(0);
            let next_allowed = last.saturating_add(self.min_interval_ms);
            map.insert(host_key.to_string(), now.max(next_allowed));
            next_allowed.saturating_sub(now)
        };
        if wait > 0 {
            tokio::time::sleep(Duration::from_millis(wait)).await;
        }
    }

    /// GET 文本，带重试
    pub async fn get_text(&self, host_key: &str, url: &str) -> Result<String> {
        let mut attempt = 0u32;
        loop {
            let _permit = self.permits.acquire().await.map_err(|_| Error::Cancelled)?;
            self.throttle(host_key).await;
            self.stats.requests.fetch_add(1, Ordering::Relaxed);

            match self.client.get(url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || status.is_server_error()
                    {
                        drop(_permit);
                        if attempt >= 4 {
                            return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                        }
                        self.stats.retries.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                    }
                    let body = resp.text().await?;
                    // 软限流：返回 200 但内容是错误信封
                    if RATE_LIMIT_MARKERS.iter().any(|m| body.contains(m)) {
                        self.stats.throttled.fetch_add(1, Ordering::Relaxed);
                        if attempt >= 4 {
                            return Err(Error::RateLimited);
                        }
                        self.stats.retries.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    return Ok(body);
                }
                Err(e) => {
                    if attempt >= 4 {
                        return Err(e.into());
                    }
                    self.stats.retries.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(backoff(attempt)).await;
                    attempt += 1;
                }
            }
        }
    }

    /// POST 原始字节并返回响应字节。
    ///
    /// 用于酷我 `yl_res_manage.search` 这类接口：请求体是 base64 文本，
    /// 响应也是 base64 文本，**不能用 `.text()`**（reqwest 会按 UTF-8 解码后
    /// 丢失原始字节），必须走 `bytes()`。
    pub async fn post_bytes(
        &self,
        host_key: &str,
        url: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<Vec<u8>> {
        let mut attempt = 0u32;
        loop {
            let _permit = self.permits.acquire().await.map_err(|_| Error::Cancelled)?;
            self.throttle(host_key).await;
            self.stats.requests.fetch_add(1, Ordering::Relaxed);

            let req = self
                .client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .header(reqwest::header::CONNECTION, "close")
                .body(body.clone());

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || status.is_server_error()
                    {
                        drop(_permit);
                        if attempt >= 4 {
                            return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                        }
                        self.stats.retries.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                    }
                    return Ok(resp.bytes().await?.to_vec());
                }
                Err(e) => {
                    if attempt >= 4 {
                        return Err(e.into());
                    }
                    self.stats.retries.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(backoff(attempt)).await;
                    attempt += 1;
                }
            }
        }
    }

    /// GET 原始字节（音乐信息包的二进制响应必须用这个）
    pub async fn get_bytes(&self, host_key: &str, url: &str) -> Result<Vec<u8>> {
        let mut attempt = 0u32;
        loop {
            let _permit = self.permits.acquire().await.map_err(|_| Error::Cancelled)?;
            self.throttle(host_key).await;
            self.stats.requests.fetch_add(1, Ordering::Relaxed);

            match self.client.get(url).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status == reqwest::StatusCode::TOO_MANY_REQUESTS
                        || status.is_server_error()
                    {
                        drop(_permit);
                        if attempt >= 4 {
                            return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                        }
                        self.stats.retries.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(backoff(attempt)).await;
                        attempt += 1;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(Error::Http(format!("HTTP {}", status.as_u16())));
                    }
                    return Ok(resp.bytes().await?.to_vec());
                }
                Err(e) => {
                    if attempt >= 4 {
                        return Err(e.into());
                    }
                    self.stats.retries.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(backoff(attempt)).await;
                    attempt += 1;
                }
            }
        }
    }

    /// HEAD 探测：拿到内容长度与 content-type（用于占位文件识别）
    pub async fn probe(&self, url: &str) -> Result<ProbeInfo> {
        let resp = self.client.head(url).send().await?;
        let status = resp.status().as_u16();
        let len = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        Ok(ProbeInfo {
            status,
            bytes: len,
            content_type: ct,
        })
    }

    /// 取流（支持 Range 透传），供本地代理与下载器使用
    pub async fn get_range(&self, url: &str, range: Option<&str>) -> Result<reqwest::Response> {
        let mut req = self.client.get(url);
        if let Some(r) = range {
            req = req.header(reqwest::header::RANGE, r);
        }
        let resp = req.send().await?;
        Ok(resp)
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProbeInfo {
    pub status: u16,
    pub bytes: Option<u64>,
    pub content_type: String,
}
