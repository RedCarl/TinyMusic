# TinyMusic 仓库协作说明（面向 coding agent）

## 这是什么

一个轻量音乐客户端。本仓库只包含桌面端与官网；iOS 客户端已拆到独立的私有仓库 `RedCarl/TinyMusic-iOS`（未开源），不在本仓库维护，也不要往本仓库加 iOS 内容。

- `desktop/` — 桌面端（Svelte 5 + Tauri 2），UI 与交互是产品基线，**行为不能退化**。
  - `desktop/crates/core` — 平台无关底座（酷我 / 网易云 / S3 / 下载 / SQLite / 文件缓存 / ID3）。同时被私有 iOS 仓库以 git submodule 引用，是共享 core 的唯一来源。
  - `desktop/crates/app` — 桌面业务层：AppState + dispatch(cmd, args)。
  - `desktop/crates/server` — 本地 HTTP API / S3 桩。
  - `desktop/packages/shared` — TS 公共库：类型、api 契约、transport、进度、纯逻辑、PlayerController。
  - `desktop/packages/theme` — 设计令牌单一来源与生成器。
  - `desktop/site` — 官网与隐私政策（GitHub Pages），含 App Store 在用的 iOS 支持/隐私/条款页。
  - `desktop/tools`、`desktop/docs`。

## 铁律

1. **一个 bug 只修一处**：S3 解析/签名在 `desktop/crates/core::library`，收藏（`favorites.json` 清单是收藏本体，「喜欢」目录只兜底存网盘里没有的歌）在 `desktop/crates/core::favorites`；桌面业务规则在 `desktop/crates/app` 或 `desktop/packages/shared`。不要在桌面端再写一套 S3 解析。
   - `packages/shared` 只放框架无关代码：不要 import svelte。
   - 平台差异用注入解决（S3 client、播放引擎、KV），不要用 `if (isMobile)` 散在主逻辑里。
   - core 被私有 iOS 仓库以 submodule 固定引用：改 core 后要在 iOS 仓库跑 `./scripts/sync-core.sh` 更新指针，或提醒维护者同步。
2. **桌面端行为锁定**：改 desktop 内部接线可以，UI/交互/文案不能变。改完跑 `desktop/docs/桌面回归清单.md`。
3. **JSON 契约唯一**：桌面命令以 `desktop/packages/shared/src/api/types.ts` + `desktop/crates/app/src/dispatch.rs` 为准。
4. **官网路径稳定**：`desktop/site/tinymusic/{support,privacy,terms}.html` 是 App Store 正在使用的外部链接，路径不能改、页面不能删。

## 常用命令

    cd desktop && pnpm install                 # desktop 是 pnpm workspace 根
    cd desktop && pnpm dev                     # 桌面前端（vite，:1420）
    cd desktop && pnpm tauri dev               # 桌面完整应用
    cd desktop && pnpm build                   # 桌面前端产物
    cd desktop && cargo build -p tinymusic     # 桌面壳（Tauri）
    cd desktop && cargo test -p tinymusic-core # 协议层测试
    cd desktop && cargo test --workspace       # 全 workspace 测试
    cd desktop && cargo run -p tinymusic-server --example s3_stub -- --port 9100 --dir /tmp/tm-s3stub

## 私有 iOS 仓库

- 源码、截图、App Store 资料都在 `RedCarl/TinyMusic-iOS`（private），本仓库不包含。
- iOS 只通过 submodule 依赖 `desktop/crates/core`；不要为了 iOS 在本仓库加构建脚本或工作流。

## 已知约束

- 桌面端支持 macOS 11+；老引擎（macOS 11/12 的 WKWebView）用的 sRGB 兜底由 `python tools/srgb-fallback.py --check` 校验。
- 桌面端行为回归清单：`desktop/docs/桌面回归清单.md`。
