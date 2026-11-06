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

跑网盘（S3）集成测试（用你自己的桶，不留密钥在仓库里）：

```bash
S3_EP=https://your-s3.example.com S3_BUCKET=your-bucket \
S3_AK=... S3_SK=... \
cargo test -p tinymusic-core --test s3_live -- --ignored --nocapture
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
settings_get/set(settings)                       // 设置（含网盘与托盘开关）
open_download_dir()                              // 打开下载目录
s3_test()                                        // 网盘连通性
s3_playlists / s3_playlist_create/delete         // 网盘歌单
s3_download / s3_download_batch                  // 下载并上传到网盘
hide_to_tray()                                   // 收进托盘
```

艺人页与专辑页**不需要额外端点**：用搜索结果按 `ARTISTID` / `ALBUMID` 归组。
实测搜「周杰伦」返回 50 条中 50 条 `ARTISTID=336`，归出 18 张专辑。

---

## 播放器：状态、随机与循环

播放按钮不再是「一个图标切两种颜色」，而是按真实状态走：

| 状态 | 按钮表现 | 无障碍标签 |
|---|---|---|
| `loading` | 旋转的加载图标 | 加载中 |
| `playing` | 暂停图标 | 暂停 |
| `paused` | 播放图标 | 继续播放 |
| `ended` / `error` | 播放图标 | 重新播放 / 重试 |

状态由 `<audio>` 的 `play` / `pause` / `ended` 事件回写（`syncPlayback`），
不在 UI 里硬写，所以系统暂停、播放失败、切歌都能正确反映。

**播放器栏**（Apple Music 风格）：

- 没有播放中的曲目时整条播放器隐藏；开始播放时从底部滑入，主区域底部留白同步展开
- 左侧五个键：随机 / 上一首 / 播放·暂停 / 下一首 / 循环
- 中间：封面 + 曲名·歌手，下方一条**细进度线**；悬停线条才出现滑块和时间，
  点击/拖动可跳转，平时不占横向空间
- 右侧三个键：歌词 / 播放列表（带曲目数徽标）/ 音量
- 下载、音质（标准 / 高品 / 超高）、歌曲详情、专辑和歌手入口放在**播放器右键菜单**里（不再单列「…」按钮）
- 播放列表面板不再重复放随机/循环开关，播放顺序统一由播放器左侧的两个键控制；面板出现/收起带过渡，点空白关闭，没有黑色遮罩

**随机和循环是两个独立开关**，和 Apple Music 一致：

- 随机播放：开/关；开启后下一首随机挑（避免连续重复；上一首也是随机）
- 循环：不循环 → 列表循环 → 单曲循环 → 不循环；单曲循环时 `ended` 原地重开

两个开关分别持久化在 `localStorage.shuffle` / `localStorage.repeat`，
旧版本 `play_mode` 会自动迁移过来。

**音量**：点播放器右侧的喇叭，音量滑杆向左展开，鼠标离开后自动收起；
必须按住并拖动才会改音量（悬停、单击不会跳变），拖到 0 时图标变为静音，
正常音量下按大小切换「小音量 / 大音量」图标。

**封面**：列表缩略图用 120px，详情页、歌词页、专辑页会通过 `coverUrl(url, size)`
把酷我 URL 里的尺寸段换成 500/800，拿同一张图的高清版，不再糊。

**播放记录**：当前曲目、队列（上限 500 首）、队列位置、进度和时长会写到
`localStorage.playback`。播放中每 2 秒、暂停、切歌、进度跳转、页面隐藏时保存；
清空播放列表会一并清掉记录。

下次打开应用时只恢复「暂停中的上次曲目和进度」，**不会自动播放**：
播放器停在最后的进度上，点播放才从该位置取流继续。恢复进度靠近结尾时从头开始，
避免一按播放就结束。

## 界面外壳（参考 Apple Music）

整体骨架按 Apple Music 的分区方式重排：

- 左侧栏分为「搜索 / 资料库 / 其他」三块，底部是账号行；选中项是一块圆角底色，
  图标用强调色，不再用左侧的色条
- 播放器从底部整条状态栏改成**悬浮毛玻璃胶囊**：宽度封顶 800px，
  居中压在内容区上方，列表和分页可以滚到它下面；主区域底部留白按播放器高度收窄到 78px，加上页面自身 32px 即可，不在有/无播放器时都留一大块空位
- 播放队列浮层跟着抬高到胶囊上方，不会和播放器叠在一起
- 列表窗口变窄时逐级收列：≤1020px 收起专辑列，≤940px 再收起热度列，
  保证标题列在 880px 最小窗口下仍有空间
- 页面标题统一放大到 28px，页面内边距同步放宽，接近 Apple Music 的内容区留白；
  内容区顶部只保留页面自己的 22px，不再叠加 macOS 标题栏的 46px inset（红绿灯只在侧边栏上方）

**macOS / Windows 的窗口不同**：

- macOS 用 Tauri 的 `titleBarStyle: "Overlay"`（`hiddenTitle` + 自定义红绿灯位置），
  红绿灯浮在侧边栏顶部；侧边栏顶部留 46px 并放了一个 `data-tauri-drag-region`
  拖动条，整块区域可以拖动窗口。窗口能力里补了 `core:window:allow-start-dragging`
- Windows 继续用系统原生标题栏（右上角最小化/最大化/关闭），不做自绘；
  侧边栏顶部不加留白，内容从窗口顶部开始
- 前端在 `main.ts` 里根据 UA + 是否存在 `__TAURI_INTERNALS__` 给 `<html>`
  打上 `data-os` / `data-shell`，CSS 只对「macOS + Tauri」加 `--titlebar-inset`；
  这个 inset 只给侧边栏拖动条和歌词页用，内容区不再重复叠加，浏览器里调试也不会多出空白

## 加载骨架

骨架屏不再是所有页面共用一个「缩略图 + 两行文字」的通用列表，而是按页面结构分别实现：

- 搜索 / 喜欢 / 专辑 / 歌单：曲目行直接复用真实 `.row` 的网格列，窗口变窄时和真实列表一样收起专辑、热度列
- 专辑 / 歌单：带封面、标题、信息行、批量操作条和曲目列表
- 艺人：圆形头像、资料、简介、标签页和曲目列表
- 下载：保留真实表头，按 6 列表格排列
- 网盘：歌单行 / 歌单详情行分别对应真实结构
- 账号歌单：名称 + 首数 + 导入按钮
- 歌曲详情：歌词区域的行骨架

显示时机由 `delayedVisible` 统一控制（`src/lib/loading.svelte.ts`）：
**加载超过 160ms 才显示骨架**，一旦显示**至少保留 320ms**。
快速请求不会出现「骨架一闪而过再换内容」；慢请求则能看到和最终页面对齐的骨架。

## 右键菜单

歌曲、专辑、歌单、下载记录、歌词……凡是「有操作可做」的地方都支持右键，
菜单由 `<ContextMenu />` 统一渲染（自动避开窗口边缘，Esc / 点空白 / 滚动即关闭），
轻反馈走右下角的 toast，不再弹 alert。

| 位置 | 菜单项 |
|---|---|
| 歌曲行 | 播放 / 下一首播放 / 添加到播放列表（已在列表时置灰）/ 下载（含歌词与封面）/ 喜欢或取消喜欢 / 查看详情 / 打开歌手 / 打开专辑 |
| 播放列表行 | 同上 + 从播放列表移除 |
| 搜索结果历史 | 搜索「关键词」/ 删除这条记录 / 清空搜索历史 |
| 专辑卡片（搜索、艺人页） | 打开专辑 / 播放全部 / 下载全部 |
| 专辑页、歌单页头部 | 播放全部 / 下载全部 |
| 下载记录行 | 打开下载目录（仅本地）/ 复制文件路径 / 重新下载 |
| 账号歌单行 | 打开歌单 / 一键导入到网盘 / 复制歌单 ID |
| 播放器任意位置（右键） | 查看歌词 / 下载当前曲目 / 切换音质 / 歌曲详情 / 打开专辑 · 歌手 |
| 歌词行 | 从这一句开始播放 / 复制这句歌词 |

「喜欢」状态会先读本地库再决定菜单文案（喜欢 / 取消喜欢），改完立即同步行内红心。

## 搜索页：状态缓存与历史查询

搜索结果放在模块级 `$state`（`src/lib/searchStore.svelte.ts`）：
切到别的页面再点回「搜索」，关键词、结果、页码、艺人专辑区、滚动位置全部原样恢复，
不会重新发请求。

搜索历史存在 `localStorage.search_history`（最多 20 条，最近在前），
搜索栏下方以 chip 展示，可单条删除或清空。

## 艺人页

歌手页除曲目/专辑外，还会拉取 PC 歌手页里的资料：

- 头像（`pic300` → 通过 `coverUrl` 升到 500px，失败时退回歌手写真接口）
- 基本资料：外文名 / 生日 / 地区 / 出生地 / 身高 / 体重 / 星座 / 语言
- 简介：默认折叠 5 行，可展开

解析在 `core/src/providers/kuwo/mod.rs::parse_singer_page`，
结果是 24h 内存缓存，不会每次进页面都抓 200KB 的 HTML。

## 歌词页（Apple Music 风格）

点播放器最左侧的封面进入歌词页（`Esc` 或「返回」退出）：

- 布局参考 Apple Music：左侧是封面、歌名/歌手/专辑、播放控制与进度，
  右侧是占满高度的歌词，当前行始终停在这一列的正中间
- 歌词按「当前行距离」分层：当前行放大加粗并带一点光晕，相邻行渐暗，
  顶部与底部做渐隐遮罩；换行时用带缓动的滚动，不硬切
- 用户手动滚歌词时暂停跟随，停手约 3 秒后自动回到当前行
- 点任意一句直接跳转，右键可复制单句
- 打开/关闭是从下往上滑入、向下滑出（尊重系统 reduced-motion）
- 背景是封面模糊图，深浅主题各有一套遮罩与文字色：
  深色压暗、浅色提亮磨砂，不会再出现浅色主题下歌词发白看不清的问题
- 歌词加载中显示骨架屏，失败时可「重新获取」
- 播放时自动预取歌词并做内存缓存，详情接口限流也不阻塞播放

## 下载内容：封面 + 歌词

每首歌下载完成后：

- **封面写进音频文件**：ID3v2.3 的 `APIC` 帧（front cover / image/jpeg），
  换电脑、传到手机、车载播放器里都自带封面
- **歌词另存同名 `.lrc`**：UTF-8 BOM，带 `[ti:][ar:][al:]` 头，主流播放器可识别

歌词从详情接口取，封面从 CDN 取；两者都是「尽力而为」——
拿不到就只下音频，不会因为附加内容失败而让整首歌失败。

## 网盘（S3）

设置页开启后，点任何「下载」都会先弹窗：**下载到本地** 还是 **下载到网盘**。

- 网盘歌单 = 桶内前缀目录，支持新建/选择，批量与整张专辑下载会一次问清后顺序上传
- 上传内容：音频（内嵌封面）+ 同名 `.lrc` + 封面 `.jpg`
- 兼容 AWS S3 / MinIO / Cloudflare R2：Path-Style 与 Virtual-Hosted-Style 都支持
- 签名是手写 SigV4（`sha2` + `hmac`），不引 aws-sdk
- 下载记录里的路径是 `s3://bucket/key`

S3 客户端在 [`core/src/s3.rs`](core/src/s3.rs)，
歌单记录在 SQLite `s3_playlists` 表（只存名字，删歌单不动云上文件）。

**网盘页**（侧边栏「网盘」）只做两件事：看歌单、看歌里的歌。

- 列表：每个歌单一行（左侧拖拽把手 + 封面 + 名称 + 曲目数），右侧三个小按钮：播放整张 / 重命名 / 删除；
  歌单支持按住拖动排序，顺序存在本地 SQLite（`s3_playlists.sort_order`），重启后保持；
  「喜欢」是红心管理的保留歌单，只可播放。
- 详情：点进歌单看歌曲列表，支持播放全部、单曲播放/暂停、删除单曲、重命名、删除歌单。
- 新建歌单是弹窗输入；删除歌单会二次确认，并**连同云端文件一起删除**。
- 重命名会把该目录下的对象逐个搬到新目录（GET→PUT→DELETE；RustFS 的 CopyObject
  签名与 AWS 不一致，服务端复制用不了）。
- 播放走 6 小时有效的 SigV4 签名直链（`presign_get`），不经过平台取链；
  歌词取同目录 `.lrc`，封面取同名 `.jpg/.png/.webp`。

设置页只保留网盘连接参数，不列出歌单。

**账号歌单一键导入**：账号页每个歌单右侧的云图标 → 选目标网盘歌单（默认同名）→
逐首「取流 → 下载（含封面/歌词）→ 上传」，带「第 N / 共 M 首」进度；
中途失败的曲目会跳过并计数，不会因为一首失败中断整张歌单。

## 滚动条

Windows 的 WebView2 默认滚动条又宽又亮，和深色界面非常割裂。
现在全局统一成 8px 细滚动条：透明轨道、圆角滑块、无上下箭头，
同时写了 `scrollbar-width: thin` / `scrollbar-color`，Firefox 与新版 Chromium 也一致。

## 托盘与关闭行为

- 关闭主窗口 → **收进系统托盘，播放不中断**（设置里可关掉这个行为）
- 托盘菜单：显示主界面 / 播放暂停 / 上一首 / 下一首 / 退出
- macOS 点 Dock 图标也能把隐藏的窗口叫回来（`RunEvent::Reopen`）
- 关掉「关闭时收进托盘」后，关闭窗口即退出，不留下没有窗口的进程

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

用到的图标：Search01 · **Heart（爱心）** · Download01 · FolderDownload（侧边栏下载）·
Settings01 · MusicNote01 · Play · Pause · Loading03 · Previous · Next · Volume ·
ArrowLeft01 · FolderOpen · Shuffle · Repeat · RepeatOne01 · ListMusic · Queue01 ·
ChevronUp · Cancel01 · CloudUpload · History · Trash · Disc · Quote。

侧边栏曾出现「下载和设置是同一个齿轮」的 bug：`features` 异步加载后导航项插队，
未加 `key` 的 `{#each}` 复用了按钮，`HugeiconsIcon` 的 `icon` 属性没跟着更新。
现在导航 `{#each nav as n (n.name)}` 强制按名字重建。

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
- **「喜欢」存在网盘里**（只有网盘启用时才出现）：点歌曲红心 = 写入本地缓存 +
  上传 `前缀/favorites.json` + 把这首歌下载到网盘 `前缀/喜欢/` 目录。
  换设备后打开 App 会自动 `favorites_pull` 合并云端列表。
- **登录只是导入工具**：账号页登录后列出平台歌单，一键导入网盘；
  喜欢不再依赖账号，也不再走平台的 `MYFAVORITE` 歌单。
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

## 搜索热度和账号清理

- 搜索结果每首歌显示热度（酷我搜索的 SCORE100），列表按热度从高到低排序。
- 退出登录会清掉酷我网页登录态的 cookie 与 localStorage，换账号不会再沿用上一个账号。
- 下载记录支持「清除记录」：只清空列表，不删除本地/网盘文件。
- 登录窗口里的 QQ / 微信扫码页通过 `on_new_window` 打开成置顶子窗口。

## 杀软误报与签名

网盘下载早期版本会把音频先写到 `%APPDATA%/com.tinymusic.app/s3tmp` 再删除，
部分国产杀软（如 360）会把「媒体文件被创建后删除」判定为勒索行为。

现在上传走内存：`Downloader::fetch_bytes` 直接下载到内存并打标签，不再产生临时 mp3，
启动时也不再递归删除旧目录，只尝试删空目录。

如果仍被拦截：

1. 安装包/主程序做 Windows 代码签名（Authenticode）。有证书后可在 `tauri.conf.json`
   配置 `bundle.windows.certificateThumbprint`，或在 CI 里用 `signtool` 签
   `tinymusic.exe`、安装包和 MSI；EV 证书可立即消除 SmartScreen 提示。
2. 向杀毒厂商提交误报：360 开放平台「误报申诉」、微软 Defender 样本提交，
   附上版本号与文件哈希，一般 1~3 个工作日处理。
3. 用户侧临时处理：把安装目录与 `%APPDATA%/com.tinymusic.app` 加入信任区。

## 外观主题

设置 → 外观提供「跟随系统 / 浅色 / 深色」三档，默认跟随系统，选择存在 localStorage。
切换会同时更新应用内配色和窗口标题栏（`set_window_theme`），系统主题变化时自动跟随。
窗口标题栏在 macOS 上是 Overlay（默认浅色/深色由系统决定），Windows 上使用原生标题栏配色。
