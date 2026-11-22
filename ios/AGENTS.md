# ios/ 子树说明（原生 iOS S3 播放器）

## 结构

    ios/
    ├─ project.yml            XcodeGen 描述；改工程结构只改它
    ├─ Sources/               App / Backend / Library / Cache / Player / Features / DesignSystem
    ├─ Tests/                 TinyMusicTests（Swift Testing）+ TinyMusicUITests（XCUITest）
    ├─ Supporting/            Info.plist、entitlements、C 头文件
    ├─ Resources/             Assets、PrivacyInfo.xcprivacy
    ├─ scripts/               bootstrap / build / test / run-simulator / screenshot / seed-s3-stub / seed-demo-library / archive / appstore-screenshots / install-device / testflight
    ├─ docs/                  验收记录 / TestFlight / App Store 资料
    └─ crates/ffi             Rust FFI；共享 core 以路径依赖 ../desktop/crates/core

## 命令

    cd ios
    ./scripts/bootstrap.sh                       # xcodegen generate
    ./scripts/build.sh                           # 编译模拟器 App
    ./scripts/test.sh                            # 起 S3 桩 + Swift 单测
    ./scripts/run-simulator.sh                   # 安装并启动 iPhone 模拟器
    ./scripts/screenshot.sh /tmp/a.png
    ./scripts/archive.sh                         # 无签名 Release archive
    ./scripts/seed-demo-library.sh               # 演示歌单（截图/审核）
    ./scripts/appstore-screenshots.sh            # 6.9 / 6.7 / 6.1 三种尺寸
    ./scripts/install-device.sh                  # 开发签名直装 iPhone
    BUILD_NUMBER=2 ./scripts/testflight.sh       # 上传 TestFlight（需 Xcode 账号）

## 规矩

- **原生 UI**：SwiftUI + 系统组件；Liquid Glass 只在 iOS 26+ 用 glassEffect，18–25 走 regularMaterial 退路。
- **S3-only**：iOS 只链接 `ios/crates/ffi`（core + ffi）；core 通过路径依赖 `../desktop/crates/core` 共享，不要 import `desktop/crates/app`、`desktop/crates/server`、provider 代码。
- **缓存是内部离线播放**：不可导出、不可分享、不出现在“文件”App、不进入 iCloud 备份。
- **业务规则不要两端重写**：S3 解析/签名/收藏（`favorites.json` 清单是收藏本体，「喜欢」目录只放网盘里没有的歌）在 tinymusic-core；队列规则在 PlaybackQueue；改一处要同步测试。
- **密钥**：AK/SK 只进 Keychain（可 iCloud 同步），不落 settings.json；日志不要打印完整 presigned URL。
- **测试**：Debug 启动参数以 -uiTest 开头，Release 忽略；测试默认 iPhone 18 Pro / iOS 27.2，兼容回归 iPhone 16 Pro / iOS 18.5。
