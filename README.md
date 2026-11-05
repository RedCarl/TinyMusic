# TinyMusic

轻量级桌面音乐客户端。Tauri 2 + Svelte 5 + Rust，**不内置登录、不做签名请求**，
只实现酷我的匿名公开接口，协议层独立成 `core` crate，可脱离 UI 单独测试。

> 设计立场：**宁可显示「不可用」，也不假装能播。**
> 未授权曲目会明确标注并置灰，不会卡在「正在搜索」。

---

## 目录结构

```
tinymusic/
├─ core/                     # 协议核心，零 UI 依赖，可单独 cargo test
│  ├─ src/
│  │  ├─ kwjson.rs           # 酷我「伪 JSON」递归下降解析器
│  │  ├─ text.rs             # 多层转义 + HTML 实体清洗
│  │  ├─ http.rs             # 限速 / 退避重试 / 软限流识别
│  │  ├─ providers/kuwo/     # 端点、字段映射、占位识别、音质降级
│  │  ├─ proxy.rs            # 本地流代理（Range 透传，手写 HTTP/1.1）
│  │  ├─ download.rs         # 流式下载 + 校验 + 文件名清洗
│  │  ├─ id3.rs              # ID3v2.3 标签（UTF-16）
│  │  └─ storage.rs          # SQLite：曲目 / 下载 / 收藏
│  └─ tests/
│     ├─ fixtures/           # 真实响应样本
│     └─ live.rs             # 联网集成测试（#[ignore]）
├─ src-tauri/                # Tauri 外壳：16 个 IPC 命令 + 状态装配
└─ src/                      # Svelte 5 前端
```

---

## 快速开始

```bash
pnpm install
pnpm tauri dev          # 开发（自动起 vite + Rust）
pnpm tauri build        # 打包
```

只跑协议层测试（不依赖 Tauri，秒级）：

```bash
cargo test -p tinymusic-core
```

跑联网集成测试（真打接口）：

```bash
cargo test -p tinymusic-core --test live -- --ignored --nocapture
```

---

## 逆向结论：这个客户端的边界在哪

全部结论来自对酷我 PC 8.7.4 / 9.6 的实测，不是推测。

### 取链协议

| 能力 | 端点 |
|---|---|
| 搜索 | `search.kuwo.cn/r.s?client=kt&all={kw}&pn={page}&rn={limit}&ft=music&rformat=json&encoding=utf8&vipver=1` |
| 取流 | `antiserver.kuwo.cn/anti.s?type=convert_url3&rid=MUSIC_{id}&format=mp3&response=url&br={br}` |
| 详情/歌词 | `m.kuwo.cn/newh5/singles/songinfoandlrc?musicId={id}` |
| 专辑封面 | `img1.kuwo.cn/star/albumcover/{web_albumpic_short}` |
| 艺人头像 | `img1.kuwo.cn/star/starheads/{web_artistpic_short}` |

CDN 直链**无需任何 header**，**支持 Range（206）**，所以本地代理可以做进度条拖动。

### 三个必须知道的坑

1. **搜索返回的不是 JSON**，而是 Python dict 字面量（单引号、未加引号的整数）。
   `core/src/kwjson.rs` 实现了递归下降解析器；不能靠「替换引号」糊弄，
   因为值里存在 `&apos;`、`&nbsp;` 和四层反斜杠（`\\\\u0026`）。
2. **匿名档位封在 128k**。请求 `320kmp3` / `2000kflac` 时，服务端对受限曲返回
   一个**固定占位文件**：181,521 字节 / 11.311 秒 / 路径含 `588957081` /
   md5 `43fe5f6460e49afdf0bc2340012fe7b5`。而且这是**按 (歌曲, 音质) 确定性**返回的。
3. **`payInfo` 不可信**。稻香的 `down='1111'`（四档全允许）实测仍只给占位。
   所以可用性只能以「实际取链 + 大小/时长校验」为准 —— 这就是 `is_placeholder()` 的三级判定。

### 为什么不做登录

`convert_url_with_sign` 需要账号 token 签名，签名算法在服务端。客户端侧无论怎么改，
服务端仍按真实权益下发 —— 这一点在 8.7.4→9.6 之间被酷我用协议换代彻底堵死
（8.7.4 用无签名 `convert_url`，9.6 全量改为 `convert_url_with_sign`）。

因此本客户端**只做匿名能拿到的部分**，并把拿不到的部分诚实地标出来。

---

## 可用性模型

| 状态 | 含义 | UI |
|---|---|---|
| `available` | 探测通过，可取整曲 | 正常可播放/下载 |
| `preview_only` | 能取到但只是片段 | 黄色「仅试听」徽标 |
| `unavailable` | 会员专享 / 已下架 / 取链失败 | 置灰，按钮禁用 |

详情页提供「测 128k / 测 320k / 测无损」三个探测按钮，用真实结果告诉你哪一档能拿到。

---

## IPC 命令面

```rust
get_providers()                                  // 平台列表
search(provider, kw, page, limit)                // 搜索
resolve(provider, id, quality)                   // 取流（含降级）
track_detail(provider, track)                    // 详情 + 歌词
artist(provider, artist_id, name, page)          // 艺人页
album(provider, album_id, name, page)            // 专辑页
player_open(provider, id, quality)               // → 本地代理 URL
download_start(track, quality, channel)          // 下载（进度走 Channel）
download_list()                                  // 下载记录
favorite_add/remove/list/check                   // 收藏
settings_get/set(settings)                       // 设置
open_download_dir()                              // 打开下载目录
```

艺人页与专辑页**不需要额外端点**：用搜索结果按 `ARTISTID` / `ALBUMID` 归组。
实测搜「周杰伦」返回 50 条中 50 条 `ARTISTID=336`，归出 18 张专辑。

---

## 图标

统一使用 **Hugeicons 免费 Stroke Rounded**（`@hugeicons/svelte` 1.x + `@hugeicons/core-free-icons` 4.x）。

- 全部图标集中在 [`src/lib/icons.ts`](src/lib/icons.ts) 做语义映射（`icons.search`、`icons.favorite`…），
  组件里不各写各的，避免「同义不同图」。
- 尺寸只有三档：`ICON_SIZE.inline = 16`（行内操作）/ `control = 18`（导航与播放控制）/ `empty = 40`（空状态）。
- 图标名不是凭记忆写的：用 Hugeicons MCP 的 `search_icons` 解析，并用以下命令校验存在性（当前 12 个全部通过）：

```bash
grep -rhoE '\b[A-Z][A-Za-z0-9]*Icon\b' src | sort -u > /tmp/used.txt
grep -oE 'declare const \w+Icon' \
  node_modules/@hugeicons/core-free-icons/dist/types/index.d.ts \
  | sed 's/declare const //' | sort -u > /tmp/available.txt
comm -23 /tmp/used.txt /tmp/available.txt   # 必须为空
```

用到的图标：Search01 · Star · Download01 · Settings01 · MusicNote01 · Play · Pause ·
Previous · Next · Volume · ArrowLeft01 · FolderOpen。

无图标的按钮都补了 `aria-label`，装饰性图标标 `aria-hidden="true"`。

## 歌单与账号

### 歌单（可用）

| 能力 | 数据源 | 状态 |
|---|---|---|
| 歌单搜索 | `search.kuwo.cn/r.s?...&ft=playlist` | ✅ |
| 歌单详情 + 曲目 | `nplserver.kuwo.cn/pl.svc?op=getlistinfo` | ✅ 分批拉取，上限 1000 首 |
| 歌单内播放/下载 | 走已有取链链路 | ✅ |
| 按链接/ID 直接打开 | 本地解析 pid | ✅ |
| 本地收藏歌单 | SQLite `saved_playlists` | ✅ |

歌单详情返回的曲目字段是小写命名（`id`/`name`/`artist`/`albumpic`），与搜索的
`SONGNAME`/`ARTIST` 不同，所以有独立的 `map_playlist_track` 映射。
另外 `*.kwcdn.kuwo.cn` 域族已下线（实测 NXDOMAIN），封面统一经 `normalize_cdn()`
改回主域并升 https。

### 账号（架构就绪，酷我被服务端挡住）

`Session` + `AuthCapability` 是跨平台通用的：

```rust
fn auth_capabilities(&self) -> Vec<AuthCapability>;         // 声明支持哪些登录方式
async fn login_password(&self, u: &str, p: &str) -> Result<Session>;
async fn login_cookie(&self, cookie: &str) -> Result<Session>;
async fn my_playlists(&self, s: &Session) -> Result<Vec<PlaylistRef>>;
async fn logout(&self, s: &Session) -> Result<()>;
```

关键约定：**声明不支持时必须给出 `reason`**，UI 直接展示原因，
而不是放一个点了必错的假登录框。

酷我三条路都被挡住，实测依据：

| 方式 | 实测结论 |
|---|---|
| 密码 | `pc.i.kuwo.cn/US_NEW/kuwo/login_kw` 返回固定 40 字节私密密文，无 `Set-Cookie`；标准 DES(`ylzsxkwm`) 与库内 `Entrypt` 的 xor+rotate 算法均解不开 |
| Cookie | 账号接口需要 web 端 `Secret` 签名头，仅凭 cookie 无法通过 |
| 扫码 | 端点未在客户端中暴露为明文地址 |

因此「我的歌单」当前落地为**本地收藏**：搜索歌单或粘贴链接打开 → 点收藏 → 归入歌单页。

新增平台只需实现上面四个方法，命令面与账号页无需改动。

## 账号、喜欢与批量

- **账号页极简**：一个平台一张卡片（Logo + 平台名 + 登录状态 + 登录按钮）。
  新增平台只要往 `PLATFORMS` 数组里加一项。
- **「喜欢」由平台能力决定**：`MusicProvider::features()` 返回
  `ProviderFeatures { favorites: bool }`，为 `false` 时导航里不会出现该入口
  —— 不给点了必然报错的页面。酷我通过 `type=MYFAVORITE` 歌单实现，返回 `true`。
- **没有独立的「歌单」入口** —— 用户自己的歌单在账号卡片下方，登录后列出。
  公开歌单搜索入口已移除，避免和「我的歌单」混淆。
- **批量下载**：专辑页、歌单页、搜索结果、喜欢列表都有「下载全部」，
  顺序执行避免并发打爆 CDN，进度按「第 N / 共 M 首」显示。
- **搜索艺人带专辑**：搜索结果首行是横向滚动的专辑卡片，点卡片进入专辑页看全部曲目。

## 网页登录（只为拿 uid）

登录的目的只是**拿到 uid 好列歌单**，所以做成「打开官网 → 登录 → 自动取 uid」：

```
点「打开网页登录」→ Tauri 弹出 WebviewWindow 加载 www.kuwo.cn
   → 用户正常登录
   → 每 1.5s 注入一段 JS，在 cookie / localStorage / sessionStorage 里找 uid
   → 写进 `kwprobe_uid` cookie（主通道，SPA 改标题也不会丢）
   → Rust 用 `WebviewWindow::cookies()` 读走 → 关闭窗口 → 列出歌单
```

窗口体验：**普通窗口（带标题栏）、372×572（对话框本体 360×540 + 标题栏）、隐藏到就绪才显示**，
**弹在主窗口所在的那块屏幕上**（不是主屏），关掉网页里的登录框时窗口立即收起。

几个实现要点：

- 不保存任何密码/token，只留一个数字 uid
- 远程页面默认拿不到 Tauri IPC，而 `eval` 没有返回值通道，
  所以用 **cookie 作为回传总线**（比 `document.title` 可靠，路由切换不会冲掉）
- 探测有三层兜底：自建 cookie → 站点自身的 `uid`/`userid` cookie → 标题
- 自动探测失败时，页面上仍保留手动填 uid 的入口

### 登录框的真实 DOM 结构

抓下来实测（`document.querySelector('.reg_login')`）：

```
div.reg_login[992x750]        ← 全屏遮罩
  div.inner[360x540]          ← 真正的对话框本体
    div.login
      div.platforms           ← 快捷登录面板（QQ/微信/微博/酷我账号，默认显示）
      div.account             ← 帐号登录面板（含顶部页签 帐号登录/快捷登录）
```

所以隔离要以 `.inner` 为根，窗口尺寸直接取 360×540。
顶部页签属于 `.account` 面板，站点默认显示 `.platforms`，看不到页签是站点自身行为。

### 为什么用普通窗口而不是无边框卡片

无边框 + 圆角要靠透明背景实现，而 `transparent(true)` 在 macOS 上会让
WKWebView 整个不渲染（黑块或完全不可见）。改用 `macos-private-api` +
原生 `contentView.layer` 裁切，窗口依然不可见。
结论：本机不支持透明窗口，所以用普通窗口 —— 有标题栏、能拖能关，反而更省事。

### 窗口尺寸的处理顺序

窄视口下站点会切移动布局，`.reg_text`（登录/注册）根本不在 DOM 里，弹窗点不出来。
所以顺序是：**桌面宽度创建（隐藏）→ 注入点击开弹窗 → 隔离页面 → 缩到 372×552 → 显示**。
全程隐藏，用户看不到「先宽后窄」的中间态。

### 踩过的三个坑

1. **窄视口下站点会切移动布局**，「登录/注册」（`.reg_text`）被媒体查询隐藏，
   弹窗根本点不出来。解法：注入一段 CSS 把它强制显示，再点。
2. **`set_position` 收 `PhysicalPosition` 会再乘一次缩放因子**（这台机器 2x），
   窗口被推到屏幕外（x = -1460）。必须用 `LogicalPosition`。
   同样的道理，`resizable(false)` 的窗口会忽略 `set_size`。
3. **`window.center()` 在这个无边框+置顶窗口上算出了负坐标**。
   最终方案是不依赖运行时移动：创建时就用目标尺寸并 `visible(false)`，
   就绪后 `place_centered()` + `show()`，用户看不到任何中间态。

## 会员曲解锁（资源管理链路）

`anti.s` 是**账号策略**接口：匿名请求会员曲的 320k/无损时，服务端只下发固定的
11.3 秒占位文件。但客户端还有一个**不依赖账号**的取链通道，逆向还原后已接入。

### 协议

```
① GET  search.kuwo.cn/r.s?stype=musicinfo&ids=MUSIC_{rid}
        响应是自定义包: [u32 明文长][u32 压缩长][zlib 流]
        解出每种音质的: MP3H=S1:...|S2:...|SIZE:...

② POST deliver.kuwo.cn/yl_res_manage.search
        请求体 = base64(加密(明文))，明文形如:
        <001><U_QRY>|<S1,S2>|<58365624><MUSIC_9.6.0.0_W1><...exe>|<>|<rid>|...
        响应体也是 base64(加密(明文))，解出:
        <URL><1>|<http://kw-*.kuwo.cn/{token}/{ts}/resource/{mid}/trackmedia/{file}.flac>

③ GET  直链（CDN 只校验 URL 里的 token/时间戳，与账号无关，支持 Range）
```

### 加密算法

从 `lidx.dll` 反汇编得到（密钥常量在 `0x1000f564` / `0x1000f574`），
是「两轮 旋转 + 定长异或」，没有 S 盒、没有分组填充：

```rust
// 加密
x.rotate_right(133);  x ^= K13;  x.rotate_left(71);  x ^= K23;
// 解密
x ^= K23;  x.rotate_right(71);  x ^= K13;  x.rotate_left(133);

const K13: &[u8] = b"KoOtOiTvINGwd";              // 13 字节
const K23: &[u8] = b"_Y8g2E6n0E1i7L5t2IoOoNk";    // 23 字节
```

异或按 `i % key.len()` 循环取字节，旋转量对消息长度取模。

正确性由 `core/src/providers/kuwo/crypt.rs` 的
`matches_real_client_ciphertext` 保证 —— 用真实抓包向量比对，
**加密结果与客户端线上字节完全一致**。

### 实测结果

以《稻香》(440613, `PAY=16711935`) 为例，未登录：

| 音质 | 大小 | 实测 |
|---|---|---|
| 无损 FLAC | 26,012,257 | ✔ `audio/x-flac`，文件头 `fLaC` |
| 320k MP3 | 8,940,938 | ✔ |
| 128k MP3 | 3,576,668 | ✔ |

三个大小与 `musicinfo` 报的 `SIZE` 全部一致。

### 降级逻辑

`resolve_with_fallback` 里，`anti.s` 命中占位文件（或该档位无直链）时，
自动改走 `resolve_trackmedia`；成功则缓存 24 小时。
因此"会员曲"在 UI 里是**可播放**的，`PAY` 字段只作为「会员内容」提示。

## 技术要点

**本地流代理**（`core/src/proxy.rs`）
把会过期的 CDN 直链藏在后端，前端只拿 5 分钟有效的 token；
透传 `Range` 让 `<audio>` 能拖进度条；手写 HTTP/1.1，不引 hyper。

**限流与重试**（`core/src/http.rs`）
按主机令牌桶（默认 1 请求 / 700ms）、并发闸门 3、指数退避。
详情接口返回 `{"status":301}` 属于软限流，会被识别并按失败重试；
重试耗尽后详情页降级为「只用已有元数据」，不阻塞播放。

**下载防护**（`core/src/download.rs`）
先写 `.part`，落盘后校验大小/时长，命中占位特征就删文件并报 `Unavailable`，
绝不给用户留一个 11 秒的「假歌」。通过校验才写 ID3 并改名。

---

## 构建与发布

代码仓库：<https://github.com/RedCarl/TinyMusic>

推送到 `main` 后，在 Actions 里运行 **release** 工作流（或推 `v*` 标签）即可：

| 平台 | 产物 |
|---|---|
| macOS arm64 | `TinyMusic_<版本>_aarch64.dmg` + `.app.tar.gz` + `.sig` |
| macOS x64 | `TinyMusic_<版本>_x64.dmg` + `.app.tar.gz` + `.sig` |
| Windows x64 | `TinyMusic_<版本>_x64-setup.exe` / `.msi` + `.sig` |

工作流用 `tauri-apps/tauri-action`，自动创建 Release 并上传所有产物，
包括自动更新用的 `latest.json`。

需要的仓库 secret：

```
TAURI_SIGNING_PRIVATE_KEY          # ~/.tauri/tinymusic.key 的内容
TAURI_SIGNING_PRIVATE_KEY_PASSWORD # 无密码则不用设
```

> 私钥一旦丢失，已发布版本的自动更新就再也签不出新包，只能让用户手动下载。

## 自动更新

应用内 **设置 → 版本 → 检查更新**。

- 更新源：`https://github.com/RedCarl/TinyMusic/releases/latest/download/latest.json`
- 校验：minisign 签名，公钥内置在 `tauri.conf.json` 的 `plugins.updater.pubkey`
- 流程：检查 → 显示新版本号 → 下载并安装（带进度条）→ 自动重启

发新版本只需要改 `tauri.conf.json` / `package.json` / `Cargo.toml` 的版本号，
推送后跑一次工作流，旧版本客户端就能收到更新。


## 体积

| 项 | 大小 |
|---|---|
| 前端产物 | JS 105 KB + CSS 12 KB（gzip 后 ~40 KB，含 17 个 Hugeicons 与渲染器） |
| 协议核心 | 无运行时开销（含 SQLite bundled） |
| macOS .app | 4.55 MB |
| macOS .dmg | 3.16 MB |

前端零 UI 框架依赖（除 Svelte 运行时），Rust 侧不引入 anyhow/regex/chrono/hyper。

---

## 已知限制

- 未登录只能拿到平台开放的曲目；会员曲目一律标记不可用。
- 协议非公开且会变。所有端点集中在 `core/src/providers/kuwo/mod.rs`，改一处即可。
- `Quality::Standard`（L 档）目前复用 `128kmp3`。`paytagindex` 显示 L=0/H=1/S=2/F=3
  四档，L 档独立 br 值尚未实测到。
- 新增平台需单独做一轮协议逆向，工作量不可低估；注册表在 `core/src/providers/mod.rs`。

---

## 免责

本项目只实现公开 HTTP 接口，不包含、不分发任何酷我二进制或音频资源。
曲库与音频版权归平台方所有，请勿用于商业用途。
