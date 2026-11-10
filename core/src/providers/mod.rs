//! 平台注册表。新增平台：实现 `MusicProvider` 后在这里加一行。

use crate::model::MusicProvider;
use std::sync::Arc;

pub mod kuwo;
pub mod netease;

pub fn registry() -> Vec<Arc<dyn MusicProvider>> {
    // 酷我是主音源（匿名可用、取流稳定），网易云负责目录侧补全，
    // 并把酷我作为自己的跨源播放兜底（`netease` → `kuwo`）。
    let kuwo: Arc<dyn MusicProvider> = Arc::new(kuwo::KuwoProvider::new());
    let netease: Arc<dyn MusicProvider> = Arc::new(netease::NeteaseProvider::new(Some(kuwo.clone())));
    vec![kuwo, netease]
}
