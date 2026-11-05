use std::time::Duration;

/// 全 crate 统一错误类型。刻意不引 anyhow，保持体积与可匹配性。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("网络请求失败: {0}")]
    Http(String),

    #[error("服务端限流，稍后重试")]
    RateLimited,

    #[error("响应格式无法解析: {0}")]
    Parse(String),

    #[error("未找到资源: {0}")]
    NotFound(String),

    #[error("该曲目需要会员或已下架，未登录不可用")]
    Unavailable,

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("数据库错误: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("任务已取消")]
    Cancelled,

    #[error("当前平台不支持该能力: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::Http("请求超时".into())
        } else if e.is_status() {
            Error::Http(format!("HTTP {}", e.status().map(|s| s.as_u16()).unwrap_or(0)))
        } else {
            Error::Http(e.to_string())
        }
    }
}

/// 断线重试的指数退避：0.5s → 1s → 2s → 4s
pub fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1u64 << attempt.min(4)))
}
