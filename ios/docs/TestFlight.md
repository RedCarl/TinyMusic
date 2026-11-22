# TestFlight / 内部测试

> 状态时间：2026-11-22。App 记录：TinyMusic，Apple ID `6835780946`，Bundle ID `cn.carljoy.tinymusic`。

## 当前状态：已上传并可内部测试

- **Build 34** 已上传 TestFlight，版本 `1.0.2 (34)`（2026-11-22 10:03 上传，10:20 处理完成），状态 **VALID**，已进入内部测试群组 **「内部测试」**（自动分发）。内容：Pro 解锁与付费墙（ProStore / ProPaywallView / ReviewPrompt）+ 收藏改为网盘 `favorites.json` 清单（点喜欢不再整首复制，网盘里已有的歌不会再存一份；`喜欢` 目录只兜底存网盘里没有的歌）。
- **Build 33** 可作回退项：版本 `1.0.1 (33)`（2026-11-20 16:02 上传，16:11 处理完成），状态 **VALID**、内部测试 **IN_BETA_TESTING**，含新图标。
- **Build 32** 版本 `1.0.0 (32)`，处理状态 **VALID**（2026-11-19 14:00 上传完成），内含 iOS 性能审计的 11 项修复（见文末）。
- **Build 30** 版本 `1.0.0 (30)`，处理状态 **VALID**。
- 包含 **iCloud entitlement**、缓存/喜欢修复、歌单播放、播放列表面板、首启 S3 提醒和离线缓存歌单、封面加载修复、歌单播放状态、歌词页滑动返回、播放页排版、中英双语与独立语言选择页。
- 内部测试群组 **「内部测试」** 已创建，自动分发已开启。
- 已邀请测试员 `carl030101@icloud.com`（孙磊，账户持有人）。
- 构建 21 的 entitlements 已核对：
  - `com.apple.developer.ubiquity-kvstore-identifier`
  - `keychain-access-groups`
  - 无 `TM_NO_ICLOUD` 编译标志，iCloud 设置入口可用。

## 测试员怎么装

1. iPhone 安装 **TestFlight**（App Store 免费）。
2. 用 `carl030101@icloud.com` 的 Apple ID 登录 TestFlight。
3. 接受 TinyMusic 的内部测试邀请，安装最新构建 **1.0.2 (34)**（33 仍可作为回退项）。
4. 如果没收到邀请邮件：App Store Connect → TestFlight → 内部测试 →「内部测试」→ 测试员，确认状态为「已邀请」，可重新发送邀请。

内部测试不需要 Beta App Review；外部测试才需要。

## 以后上传新构建

使用 App Store Connect **Team API Key（Admin）**，无需 Xcode 账号登录：

    cd ios
    ASC_KEY_PATH="$HOME/.appstoreconnect/private_keys/AuthKey_XXXXXXXXXX.p8" \
    ASC_KEY_ID="XXXXXXXXXX" \
    ASC_ISSUER_ID="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx" \
    BUILD_NUMBER=19 ./scripts/testflight.sh

脚本默认归档 **含 iCloud** 的 `Supporting/TinyMusic.entitlements`。上传成功会看到：

    Upload succeeded
    ** EXPORT SUCCEEDED **

随后 TestFlight 处理 10–30 分钟。

## 关键配置

- App ID `cn.carljoy.tinymusic` 的 **iCloud** capability 已开启。
- 归档 entitlements：`Supporting/TinyMusic.entitlements`。
- `TinyMusicTestFlight.entitlements` 仅作为「无 iCloud」应急包保留；正常发 TestFlight 不再使用。
- 出口合规：`Info.plist` 已写 `ITSAppUsesNonExemptEncryption=false`，通常不会再提示。

## 真机直接安装（开发签名）

    cd ios
    ./scripts/install-device.sh

脚本现在默认使用含 iCloud 的 entitlements，安装后可直接验证 iCloud 同步。

## 已知限制

- TestFlight 内部测试员必须是 App Store Connect 用户；外部测试员不需要，但要 Beta App Review。
- 新增测试员：App Store Connect → 用户和访问 添加用户，再到 TestFlight 群组里勾选。
- 构建 17–32 是旧包，仍在群组里；测试请选 **33**。

## 构建 29（2026-11-18 17:34）

- 版本：**1.0.0 (29)**，已上传 App Store Connect，处理状态 **VALID**（17:35 上传完成）；分包后可在 TestFlight 里装 29。
- 语言：设置 → 语言改成独立选择页，点一行立刻生效并**留在本页**（勾选与标题当场更新），不再被弹回设置页。
- 同步修正：去掉 `MainTabView` 按语言 `.id(...)` 重建整棵 tab 树（那正是弹回的原因），标签栏文案改为跟随 `Localization` 刷新；根视图补了 `environment(\.locale)`，导航栏/日期格式等系统部件跟界面语言一起切换。
- 测试：`TinyMusicTests` 全过；`TinyMusicUITests` 7 项全过（含 `testLanguageSwitchesImmediatelyWithoutRestart` 更新后的断言：停在语言页 + 标签栏变英文）。

## 构建 30（2026-11-19 00:07）

- 版本：**1.0.0 (30)**，已上传 App Store Connect；处理状态 **VALID**，内部测试 **IN_BETA_TESTING**，自动通知已开启。
- 语言：设置 → 语言为独立选择页，切换立即生效并停留在本页；标签栏、导航栏和日期格式跟随界面语言。
- S3：`S3 网盘` 行最右侧新增「断开」按钮，只有连接正常（绿点：地址/桶/密钥齐全且最近保存或测试无报错）时才显示；断开后按钮自动消失，底部独立断开区块已移除。
- 构建：`./scripts/build.sh` 成功，`TinyMusicTests` 29 项全过。

## 构建 33（2026-11-20 16:02）

- 版本：**1.0.1 (33)**，`MARKETING_VERSION` 1.0.0 → 1.0.1，`BUILD_NUMBER=33 ./scripts/testflight.sh` 归档并上传成功（`Upload succeeded` / `EXPORT SUCCEEDED`）。
- 改动：换用新 Logo —— `ios/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png`（1024×1024、RGB 无 alpha），桌面端 `desktop/src-tauri/icons/` 同步换成同一套。
- 坑：1.0.0 已审核通过后再传 1.0.0 的构建，ASC 会报 `Invalid Pre-Release Train. The train version '1.0.0' is closed`；必须抬 `CFBundleShortVersionString`。

## 构建 32（2026-11-19 14:00）

- 版本：**1.0.0 (32)**，`BUILD_NUMBER=32 ./scripts/testflight.sh` 归档并上传成功（`Upload succeeded` / `EXPORT SUCCEEDED`）。
- 内容：iOS 性能审计落地的 11 项修复，不改 JSON 契约、不改界面文案、不动桌面端。详见 `ios/docs/perf-audit-2026-11-19.md`。
  - 播放位置拆进 `PlayerProgressRow`，now-playing 改成事件驱动
  - 下载进度改按 key 的 `@Observable` 盒子，去掉逐行 `fileExists`
  - 缓存索引 2 s 防抖、文件校验移到后台、总量增量维护
  - 收藏 O(1) 索引，封面 URL 与取色解码加缓存/移出主线程，EQ 落盘 300 ms 防抖
- 测试：`./scripts/test.sh`（`TinyMusicTests` + 本地 S3 桩）通过；`TinyMusicUITests` 8 项全过（0 失败）。
- 注意：build 31 已在 2026-11-18 19:10（PST）上传过，所以本次从 32 起。
