//! 平台注册表。新增平台：实现 `MusicProvider` 后在这里加一行。

use crate::model::MusicProvider;
use std::sync::Arc;

pub mod kuwo;

pub fn registry() -> Vec<Arc<dyn MusicProvider>> {
    vec![Arc::new(kuwo::KuwoProvider::new())]
}
