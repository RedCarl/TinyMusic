# 免费试用 30 天 + 永久解锁（TinyMusic）

## 模型

- App 价格：**免费**（ASC 已改，商店缓存生效后显示「免费」）
- 试用：首次启动起 30 天，全功能
- 到期后：一次性内购解锁，**永久可用**，无订阅、无自动续费
- 价格：中国大陆 ¥8，美国 $0.99（其他地区自动等值）
- 老用户：2026-11-21 之前购买过的用户通过 AppTransaction 自动永久解锁，不会二次收费

## App Store Connect 对象

| 项 | 值 |
|---|---|
| App ID | 6835780946 |
| 版本 | 1.0.2（PREPARE_FOR_SUBMISSION） |
| IAP ID | 6837624751 |
| Product ID | `cn.carljoy.tinymusic.lifetime` |
| 类型 | 非消耗型（一次付费、永久） |
| 状态 | READY_TO_SUBMIT（中英本地化 / ¥8·$0.99 / 审核截图 / 175 地区已配齐） |

## 提审步骤

1. 归档上传构建 **34**（project.yml 已改 1.0.2 / 34）。
2. ASC → 1.0.4 版本页 → 选择构建 19。
3. 提交页勾选 App 内购买项目「TinyMusic 完整版」。
4. 提交审核。

## 代码

- `Sources/Pro/ProStore.swift`：试用时钟（Keychain 同步）、StoreKit 2 购买 / 恢复、老用户解锁
- `OTP/Pro/ProPaywallView.swift`：统一购买页 + 设置页入口
- `OTP/Pro/ReviewPrompt.swift`：（评分入口暂未接入）
- RootView：到期锁定全屏购买页；设置页显示试用剩余天数

## 测试

- 调试强制锁定：启动参数 `-proForceLocked`（DEBUG）
- 沙箱购买需在 ASC 建 Sandbox 测试账号，或本地加 StoreKit 配置文件
