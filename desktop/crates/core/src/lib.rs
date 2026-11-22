//! tinymusic-core —— 音乐平台协议核心。
//!
//! 与 UI 完全解耦：这里只有「协议 + 数据 + 存储 + 下载 + 流代理」，
//! 不含任何 Tauri 依赖，因此可以单独 `cargo test` 验证。

pub mod cache;
pub mod download;
pub mod error;
pub mod favorites;
pub mod http;
pub mod id3;
pub mod kwjson;
pub mod library;
pub mod model;
#[cfg(feature = "providers")]
pub mod providers;
pub mod proxy;
pub mod quality;
pub mod s3;
pub mod storage;
pub mod text;

pub use error::{Error, Result};
pub use model::*;

/// 平台注册表：新增平台只需在 `providers::registry` 里加一行。
#[cfg(feature = "providers")]
pub fn default_providers() -> Vec<std::sync::Arc<dyn MusicProvider>> {
    providers::registry()
}
