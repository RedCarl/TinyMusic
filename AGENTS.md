# TinyMusic 仓库协作说明（面向 coding agent）

## 这是什么

一个轻量音乐客户端。仓库按客户端分三块，各自自包含：

- `desktop/` — 桌面端（Svelte 5 + Tauri 2），UI 与交互是产品基线，**行为不能退化**。
  - `desktop/crates/core` — 平台无关底座（酷我/网易云、S3、下载、SQLite、文件缓存、ID3）。
  - `desktop/crates/app` — 桌面业务层：AppState + dispatch(cmd, args)。
  - `desktop/crates/server` — 本地 HTTP API / S3 桩；iOS 不再链接。
  - `desktop/packages/shared` — TS 公共库：类型、api 契约、transport、进度、纯逻辑、PlayerController。
  - `desktop/packages/theme` — 设计令牌单一来源与生成器。
  - `desktop/site` — 官网与隐私政策（GitHub Pages）；`desktop/tools`、`desktop/docs`。
- `ios/` — 原生 iOS App（SwiftUI + Liquid Glass），S3 网盘播放 + 离线缓存 + iCloud。
  - `ios/crates/ffi` — S3-only C ABI（tm_start/tm_invoke/tm_stop/tm_free），iOS 直接 FFI。
  - Rust 依赖以路径指向 `../desktop/crates/core`（共享核心只有一份，不要在 ios/ 复制）。

## 铁律

1. **一个 bug 只修一处**：S3 解析/签名在 `desktop/crates/core::library`，收藏（`favorites.json` 清单是收藏本体，「喜欢」目录只兜底存网盘里没有的歌）在 `desktop/crates/core::favorites`；桌面业务规则在 `desktop/crates/app` 或 `desktop/packages/shared`；iOS 队列规则在 `PlaybackQueue.swift`。不要在 iOS 再写一套 S3 解析。
   - `packages/shared` 只放框架无关代码：不要 import svelte 或 SwiftUI。
   - 平台差异用注入解决（S3 client、播放引擎、KV），不要用 `if (isMobile)` 散在主逻辑里。
2. **桌面端行为锁定**：改 desktop 内部接线可以，UI/交互/文案不能变。改完跑 `desktop/docs/桌面回归清单.md`。
3. **JSON 契约唯一**：桌面命令以 `desktop/packages/shared/src/api/types.ts` + `desktop/crates/app/src/dispatch.rs` 为准；iOS 命令以 `ios/crates/ffi/src/lib.rs` + `ios/Sources/Backend/Models.swift` 为准。
4. **iOS 只链接 core + ffi**：不要 import `desktop/crates/app`、`desktop/crates/server` 或 provider 代码；`cargo tree -p tinymusic-ffi -e features` 不得出现 `providers`。
5. **缓存不是下载**：iOS 缓存不可导出/分享/文件 App 可见/进 iCloud 备份；UI 文案用“缓存”。

## 常用命令

    cd desktop && pnpm install                 # desktop 是 pnpm workspace 根
    cd desktop && pnpm dev                     # 桌面前端（vite，:1420）
    cd desktop && pnpm tauri dev               # 桌面完整应用
    cd desktop && pnpm build                   # 桌面前端产物
    cd desktop && cargo build -p tinymusic     # 桌面壳（Tauri）
    cd desktop && cargo test -p tinymusic-core # 协议层测试
    cd ios && ./scripts/build.sh               # 原生 iOS 模拟器构建
    cd ios && ./scripts/test.sh                # Swift 单测 + S3 桩
    cd desktop && cargo run -p tinymusic-server --example s3_stub -- --port 9100 --dir /tmp/tm-s3stub
    cd ios && cargo build -p tinymusic-ffi --target aarch64-apple-ios-sim

## iOS 原生开发

- 工程由 `ios/project.yml`（XcodeGen）生成；不要手改 `TinyMusic.xcodeproj`。Rust 静态库由 Xcode Build Phase 在 `ios/target` 下编译。
- Liquid Glass 只在 iOS 26+ 用 `glassEffect`；iOS 18–25 必须走 `regularMaterial/ultraThinMaterial` 退路。
- S3 音频用 AVPlayer 直连 presigned URL；不要把本地 HTTP 服务重新加回来。
- 缓存用 URLSession background；索引在 `Application Support/tinymusic/Offline/index.json`，并排除 iCloud 备份。
- 测试默认 iPhone 18 Pro / iOS 27.2；兼容回归 iPhone 16 Pro / iOS 18.5。

## 已知约束

- iOS 只支持 S3 协议兼容存储；不接阿里云盘/百度网盘私有 API。
- 缓存不可导出/分享；不要加“下载到文件”的入口。
- iCloud v1 只同步凭据/非敏感配置；缓存不同步。
- S3 密钥只进 Keychain，不落 `ios-settings.json`。

## 验收入口（iOS 原生）

- 构建：`cd ios && ./scripts/build.sh`
- 单测：`cd ios && ./scripts/test.sh`
- 本地 S3 桩：`cd desktop && cargo run -p tinymusic-server --example s3_stub -- --port 9100 --dir /tmp/tm-s3stub`，再在 `ios/` 下 `./scripts/seed-s3-stub.sh`
- 截图：`ios/scripts/screenshot.sh /tmp/a.png`
