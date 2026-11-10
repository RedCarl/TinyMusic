# TinyMusic

轻量级桌面音乐客户端。Tauri 2 + Svelte 5 + Rust，**不内置登录、不做签名请求**，
只实现酷我的匿名公开接口，协议层独立成 `core` crate，可脱离 UI 单独测试。

> 设计立场：**宁可显示「不可用」，也不假装能播。**
> 未授权曲目会明确标注并置灰，不会卡在「正在搜索」。

**[官网 / 下载](https://redcarl.github.io/TinyMusic/)** —— Windows、macOS 安装包都在这一页，不用翻 Releases。

---

## 目录结构

```
tinymusic/
├─ core/                     # 协议核心，零 UI 依赖，可单独 cargo test
│  ├─ src/
│  │  ├─ kwjson.rs           # 酷我「伪 JSON」递归下降解析器
│  │  ├─ text.rs             # 多层转义 + HTML 实体清洗
│  │  ├─ http.rs             # 限速 / 退避重试 / 软限流识别 / 流探测
│  │  ├─ quality.rs          # 音质能力解析、文件头/时长校验、实际格式
│  │  ├─ providers/kuwo/     # 端点、字段映射、占位识别、音质降级
│  │  ├─ proxy.rs            # 本地流代理（Range 透传，手写 HTTP/1.1）
│  │  ├─ download.rs         # 流式下载 + 校验 + 文件名清洗
│  │  ├─ id3.rs              # ID3v2.3 标签（UTF-16）
│  │  └─ storage.rs          # SQLite：曲目 / 下载 / 收藏
│  └─ tests/
│     ├─ fixtures/           # 真实响应样本
│     └─ live.rs             # 联网集成测试（#[ignore]）
├─ src-tauri/                # Tauri 外壳：IPC 命令 + 状态装配
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
2. **`anti.s` 匿名档位封在 128k**。请求 `320kmp3` / `2000kflac` 时，服务端对受限曲返回
   一个**固定占位文件**：181,521 字节 / 11.311 秒 / 路径含 `588957081` /
   md5 `43fe5f6460e49afdf0bc2340012fe7b5`。而且这是**按 (歌曲, 音质) 确定性**返回的。
   绕过它的是资源管理链路（`trackmedia`）：匿名可拿 320k / FLAC，结果仍要做文件头和时长校验。
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

详情页提供六档探测按钮：128k / 320k / FLAC 可点，Hi-Res / 杜比全景声 / 母带直接灰色禁用；
列表接口的 `MINFO` 能判断某首曲子有没有 FLAC，没有就禁用无损，不让用户点了才失败。

---

## 音质档位与真实格式

| 档位 | wire | 目标 | 当前状态 |
|---|---|---|---|
| 标准 128k | `standard` | 128k 有损 | 可播 |
| 高品质 320k | `super` | 320k 有损 | 可播 |
| 无损 FLAC | `lossless` | FLAC 16/44.1 起 | 按曲目可播 |
| Hi-Res | `hires` | 高解析度 | 灰色，需会员 + 加密解密 |
| 杜比全景声 | `atmos` | 空间音频 | 灰色，需会员 + 加密解密 |
| 母带 | `master` | 母带级 | 灰色，需会员 + 加密解密 |

- 列表接口的 `MINFO` 决定某首曲目有没有无损；有就显示「无损」徽标，没有就禁用该档。
- 实际播放档位由文件头（`fLaC` / MP3 frame）+ 字节数/时长校验得出，**不直接相信请求参数**；
  请求无损但只拿到 320 时，UI 显示实际格式并标「已降级」。
- 后三档在酷我目录里存在（`ZPGA*` / `ZPLY` / `DTSX` / `BCMS` / `MNAC*` 等），
  但匿名公开接口只返回占位文件或明文 FLAC，拿不到加密 URL/ekey，所以当前不提供可选项。
  实测记录见 [docs/酷我音质调研.md](docs/酷我音质调研.md)。

---

## IPC 命令面

```rust
get_providers()                                  // 平台列表
search(provider, kw, page, limit)                // 搜索
resolve(track, quality)                          // 取流（含降级/实际格式）
quality_capabilities()                           // 六档能力（后三档不可用）
track_qualities(track)                           // 单曲可播档位探测
quality_set(quality)                             // 只改默认音质
track_detail(provider, track)                    // 详情 + 歌词
artist(provider, artist_id, name, page)          // 艺人页
album(provider, album_id, name, page)            // 专辑页
player_open(track, quality, force_refresh)       // → 本地代理 URL（含实际格式）
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
- 下载、音质（六档，当前仅 128k / 320k / FLAC 可点，其余灰色）、歌曲详情、专辑和歌手入口放在**播放器右键菜单**里（不再单列「…」按钮）
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

## 界面外壳与材质

骨架仍按 Apple Music 的分区方式：左侧栏分「搜索 / 资料库 / 其他」，底部是账号行；
播放器悬浮在内容区上方，队列浮层在它上面展开。材质换成了
[Solid Glass](https://github.com/deeplethe/solid-glass)：远看是实心的烟色玻璃，
近看才有边上的光、发丝线和阴影。完整的令牌表、组件映射和改法见
[docs/界面材质-solid-glass.md](docs/界面材质-solid-glass.md)。

- 整个窗口共用一种底色：侧边栏不铺底、不描边，只排列按钮；分组之间只留一段空气，
  没有「资料库 / 其他」标题，账号上方也不再画线
- 右侧内容是一块四周内缩 8px 的**圆角矩形**：薄薄一层底 + 一圈发丝线 + 脚下一层浅影，
  底部也能看到它的圆角边界。层次是「页面 → 内容矩形 → 浮动播放器」，
  而不是两块不同颜色的底并排
- 侧边栏收窄到 204px（菜单项都很短，不需要 224px 那么宽）
- 播放器栏浮在内容矩形内部，与矩形边缘各留 14–16px
- 面板（列表、卡片、说明块）只有边缘有光：顶上一道高光、四周一圈发丝线、
  脚下一层阴影；正面是中性灰，不铺渐变。薄 → 中 → 厚三档：
  说明块 / 卡片 / 浮层
- 浮层（弹窗、右键菜单、队列）是最厚的一档玻璃，几乎不透
- 播放器栏是唯一的**液态玻璃**：一层 24%（浅色 55%）的半透底，
  背后用 56px 模糊 + 增饱和 + 轻微提亮去折射内容；左上角一片很淡的镜面反光、
  顶边一道亮线，文字与图标各压一层淡影，透到亮封面上也读得清；
  两端是全圆的胶囊（`border-radius: 999px`）、68px 高、宽度封顶 860px，
  列表和分页可以滚到它下面。比例照 Apple Music：左侧传输键 34px（播放键 44px），
  右侧图标 22px，中间封面 48px；进度条只跟在歌名下面、最长 400px，不横跨整条中间区，
  右侧还预留了几十像素，展开音量时不会被音量药丸压住
- 内容矩形底部留白 100px，没有播放器时归零；队列浮层抬到播放器上方，不会叠在一起
- 列表窗口变窄时逐级收列：≤1020px 收起专辑列，≤940px 再收起热度列，
  保证标题列在 880px 最小窗口下仍有空间
- 窗口拉宽时内容跟着长，长到 1600px 封顶，再多的宽度用 `margin-inline: auto`
  平分到左右（而不是把内容顶在左边、右边空一大块）；行内标题列不再无限拉伸，
  徽标跟着歌名排，宽屏下也不会被甩到列尾。往窄里缩仍然是流式的，
  到 880px 以下才由上面那两级收列接手
- 页面标题 26px，内容区左右各 32px；内容区顶部只保留页面自己的 24px，
  不再叠加 macOS 标题栏的 46px inset（红绿灯只在侧边栏上方）
- 颜色只留给有意义的东西：品牌绿（正在播放、已喜欢）、成功、失败、警告、
  链接与云盘标记；主按钮、播放键、进度填充是一小块「墨」
- 深浅两套主题共用同一批令牌；系统要求减少透明时自动换成实色，
  要求减少动态时关掉动画

**macOS / Windows 的窗口不同**：

- macOS 用 Tauri 的 `titleBarStyle: "Overlay"`（`hiddenTitle` + 自定义红绿灯位置），
  红绿灯浮在侧边栏顶部；侧边栏顶部留 46px 并放了一个 `data-tauri-drag-region`
  拖动条，整块区域可以拖动窗口
- Windows 关掉原生边框（`src-tauri/tauri.windows.conf.json` 里 `decorations: false`
  + `shadow: true`，保住 Win11 的圆角和外投影），顶部换成 42px 的自绘标题栏：
  左边是品牌，整条 `data-tauri-drag-region="deep"` 都能拖动窗口、双击切换最大化，
  右边是自绘的最小化 / 最大化 / 关闭（`WindowControls.svelte`）：46px 宽、占满标题栏高度、
  贴着右上角，尺寸和位置都对齐系统标题栏，滑到屏幕角落也能直接点到关闭；
  悬停一层淡底，关闭键悬停整块变红（`#c42b1c`）。按钮固定定位、
  层级压过歌词页和弹窗，任何界面都能关窗口。
  平台配置按 JSON Merge Patch 覆盖，`windows` 数组是整体替换而不是逐项合并，
  所以这个文件把窗口尺寸也一起写全了 —— 改窗口大小时两处都要动
- 自绘标题栏只在「Windows + Tauri」出现：`main.ts` 根据 UA 和
  是否存在 `__TAURI_INTERNALS__` 给 `<html>` 打上 `data-os` / `data-shell` / `data-chrome`，
  CSS 只对 `data-chrome="custom"` 换成两行网格（标题栏 + 侧边栏/内容），
  同时收起侧边栏里那份品牌；`--titlebar-inset` 也同步给歌词页当顶部拖动条，
  浏览器里调试既不画标题栏，也不会多出空白
- 窗口能力里补了 `start-dragging` / `internal-toggle-maximize` / `minimize` /
  `toggle-maximize` / `is-maximized` / `close`；最大化图标跟着窗口的 resize 事件走，
  双击拖动条、Win+↑、贴边吸附都能正确切换到「还原」形状
- Windows 的窗口圆角是 DWM 定的 8px，右侧内容框离窗口边正好 8px，所以内容框的圆角
  也用 8px（`--panel-radius`），内外两条弧线才是平行的；用原来的 19px 时，
  内容框在右下角会鼓出来一块，和窗口的圆角「打架」。macOS 保留原来的大圆角

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

搜索结果顶部那排「艺人的专辑」是一条横向货架（`.shelf`）：不挂滚动条，
两侧各一颗圆形翻页键，滚到哪一侧才在那侧做渐隐。渐隐用的是 `mask-image`
而不是盖一层色块 —— 内容区的底是半透明玻璃，盖色永远对不上。
点一次翻一整屏：卡片占位从 DOM 里量（两张卡的 `offsetLeft` 之差），
屏幕上能放下几张就滚几张，配合 `scroll-snap` 停在整张卡上；
滚到头那一侧的按钮和渐隐一起收起。触控板横滑、Tab 到按钮都照常可用，
`smooth` 被禁时还有一次兜底跳转，按钮不会是死的。

## 艺人页

歌手页除曲目/专辑外，还会拉取 PC 歌手页里的资料：

- 头像（`pic300` → 通过 `coverUrl` 升到 500px，失败时退回歌手写真接口）
- 基本资料：外文名 / 生日 / 地区 / 出生地 / 身高 / 体重 / 星座 / 语言
- 简介：默认折叠 3 行；「展开简介」放在页签同一行的最右边，展开后变「收起简介」
- 专辑卡片有 hover：轻微上浮、一层淡色、封面提亮、专辑名变实

解析在 `core/src/providers/kuwo/mod.rs::parse_singer_page`，
结果是 24h 内存缓存，不会每次进页面都抓 200KB 的 HTML。

## 歌词页（Apple Music 风格）

点播放器最左侧的封面进入歌词页（`Esc` 或「返回」退出）：

- 布局参考 Apple Music：左侧是封面、歌名/歌手/专辑、播放控制与进度，
  右侧是占满高度的歌词。当前行会尽快停到这一列的正中间，但**上方只留一成高度（最少 72px）**：
  歌刚开始时头几句贴着上边出现，不会先空出一大片；下方留半个高度，
  保证最后几句也能滚到正中
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
逐首「取流 → 下载（含封面/歌词）→ 上传」，带真实进度条和「第 N / 共 M 首」文案；
中途失败的曲目会跳过并计数，不会因为一首失败中断整张歌单。

导入现在是**队列化**的：点「加入导入队列」后弹窗会直接关掉，任务在后台按顺序执行，
不需要守着弹窗。账号歌单页顶部的「导入队列」会显示排队中 / 导入中 / 完成 / 失败，
同一时间只跑一个任务；导入中也可以继续加别的歌单，完成后右下角有 toast 提示。
正在跑的任务不能取消，排队中和已结束的任务可以移除，已结束的可以一键清空。

## 进度反馈

下载和账号歌单导入统一使用 [`ProgressBar`](src/components/ProgressBar.svelte)：

- 进度条、旋转图标和文案是分开的三层，不会再出现「图标/文字被塞进 4px 高进度条里」的情况
- 单曲下载按字节百分比；批量下载和账号歌单导入按「已完成首数 / 总首数」推进
- 设置页的应用内更新下载也复用同一个进度条
- 刚发起、还没收到第一帧进度时显示不确定进度（来回滑动的填充），而不是空白或假满格

## 滚动条

Windows 的 WebView2 默认滚动条又宽又亮，两头的箭头小方块和深色界面非常割裂。
现在全局统一成 8px 细滚动条：透明轨道、圆角滑块、两端没有箭头。

**只写 `::-webkit-scrollbar`，不要写 `scrollbar-width` / `scrollbar-color`。**
这两个标准属性一旦声明，Chromium 就切回系统滚动条绘制，上面那套自定义样式全部失效，
Windows 上就会在滚动条两头露出小箭头 —— 「有箭头」正是这么来的。再显式把
`::-webkit-scrollbar-button` 的尺寸置零，双保险。WebView2 和 Linux 的 WebKitGTK
都认这套伪元素（项目不跑 Firefox）；歌词页那种要彻底藏掉滚动条的容器，
额外写 `scrollbar-width: none` 也不会和它打架。

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

---

## 网易云：登录、每日推荐与跨源播放

一句话：**目录用网易云，音源优先网易云直链、失败再回落到酷我**。

```text
每日推荐 / 歌单 / 搜索（网易云 weapi）
      ↓ 曲目（id / 标题 / 歌手 / 时长）
resolve()
      ├─ 1) 网易云 song/enhance/player/url/v1 → verify_url()
      └─ 2) 失败 → bridge::find_in_kuwo()（标题 0.55 + 歌手 0.30 + 时长 0.15）
                    → 酷我 resolve()（含占位文件校验）
```

播放策略可在 **设置 → 网易云（推荐与歌单）** 里切换：
`netease_first`（默认，先试网易云直链）或 `kuwo_only`（网易云只当目录）。

### 为什么需要 `weapi`

网易云的私有接口（登录、每日推荐、歌单、取流）全部走 `weapi`：

```text
POST https://music.163.com/weapi/{path}?csrf_token={__csrf}
Cookie: MUSIC_U=...
params=<base64>&encSecKey=<hex>
```

* `params` = `AES-128-CBC( AES-128-CBC(json, "0CoJUm6Qyw8W8jud"), secretKey )`
* `encSecKey` = `RSA_raw(reverse(secretKey), e=65537, n=<页面里写死的 1024bit 模数>)`
* 两个坑：外层加密的是**内层 base64 字符串本身**（不是二进制）；
  `params` 里的 `+ / =` 必须百分号编码，否则服务端直接 `code=400`。

实现在 `core/src/providers/netease/weapi.rs`（`aes` + `num-bigint`，纯 Rust，无 C 依赖），
并有单元测试覆盖 PKCS#7 补位、往返解密、`encSecKey` 长度与「每次请求换 key」。

### 登录：网页登录 + cookie 逃生

不做应用内账号密码框：网易云对异地/新设备会要求短信或设备校验（`code=8821`），
纯 HTTP 登录经常卡在这一步。

1. **网页登录（主路径）**：`账号 → 网易云 → 网页登录` 打开 `music.163.com` 的
   webview，后台每 500ms 读一次该 webview 的 cookie；读到 `MUSIC_U` 就关窗，
   用 `w/nuser/account/get` 校验一次，存库并注入 provider。
2. **cookie 登录（逃生）**：从浏览器 DevTools 复制 `MUSIC_U=...` 粘进来，
   走同一个校验接口。校验不通过会明确报「cookie 无效（缺少 MUSIC_U 或已过期）」。
3. **手机号 + 密码**：provider 里也实现了（提交 `md5(密码)`），
   遇到 `8821` 会把原因翻译给用户并建议改用上面两条路。

会话存在 `sessions` 表里，重启时由外壳 `provider.set_session()` 注入，
不需要重新登录。

### 接口与实测（2026-11-10，本机网络）

| 能力 | 接口 | 匿名 | 备注 |
|---|---|---|---|
| 每日推荐 | `v3/discovery/recommend/songs` | ⚠️ 给通用列表 | 登录后个人化，返回 30–45 首 |
| 每日推荐（旧） | `v2/discovery/recommend/songs` | ❌ `code=301` | 曲目字段是老结构（`artists`/`album`） |
| 推荐歌单 | `v1/discovery/recommend/resource` | ❌ 301 | 登录后 8–10 张 |
| 我的歌单 | `user/playlist` | ✅（公开歌单） | 含 `specialType=5` 的「喜欢的音乐」 |
| 歌单详情 | `v6/playlist/detail` | ✅ | 只给 `trackIds`，曲目要再查 `song/detail` |
| 搜索 | `cloudsearch/pc` | ✅ | `type=1` 单曲、`type=1000` 歌单 |
| 专辑 / 艺人 | `v1/album/{id}`、`v1/artist/{id}`、`artist/albums/{id}` | ✅ | `artist/album` 已废弃（400） |
| 歌词 | `song/lyric` | ✅ | 有翻译时合并成「原文 / 译文」 |
| 取流 | `song/enhance/player/url/v1` | ⚠️ 免费曲可用 | `fee=1` 会员曲返回 `code=-110` |

`fee` 语义实测：`0`/`8` 都能拿到 320k 直链，`1`/`4` 才需要账号权益。
所以 `fee=8` **不标**「仅试听」——早先把它当试听是错的。

### 跨源匹配：为什么不能只按标题搜

酷我 `search.kuwo.cn/r.s` 的排序严重偏向翻唱/DJ 版。实测搜「恋人 李荣浩」，
前 30 条里没有李荣浩原版，全是李健、李毅阳、Instrumental、雷鬼版：

```text
恋人(2013版)|李健|168s
恋人|李毅阳|240s
恋人 (Instrumental)|뎁트&慢热同学LEON|203s
...
```

所以 `bridge.rs` 做三件事：规范化标题（去括号/版本词/噪声）、校验歌手（拆
`/ & 、` 多歌手）、校验时长（>25s 直接判不同版本）。
打分 `标题 0.55 + 歌手 0.30 + 时长 0.15`，**阈值 0.75**；
候选带「伴奏/cover/Live/DJ/抖音」而目标没有，再扣 0.20。
低于阈值宁可报「不可用」，也不放错版本。

查询还会跑三轮（`标题 歌手` → `标题` → `标题 第一歌手`），取全部候选里的最高分——
上面那首李荣浩就是在补的查询里以 1.00 分命中的。

### 实测结果

`cargo test -p tinymusic-core --test netease_live -- --ignored --nocapture`

热歌榜前 20 首、匿名、320k：

```text
网易云直链 12/20 ｜ 酷我桥接 7/20 ｜ 失败 1 ｜ 合计可播 19/20（95%）
```

失败的那首是 `fee=1` 且酷我没有的曲目 —— 登录后走网易云直链即可。
单看「网易云自己不给、只能靠酷我」的那批（`fee=1`，10 首）：

```text
命中 9/10 —— 6 首标题/歌手/时长全中（1.00），3 首是标题带后缀的同一录版（0.89）
只有「Marry - Top Barry/INDEcompany」酷我确实没有
```
`bridge_matching_quality` 用例单独看匹配质量：晴天/我不难过 1.00，
天龙八部之宿敌 0.89（酷我那条标题带「《天龙八部3》网游主题曲」后缀），
明显不同版本的会判 0 分并报不可用。

### 已知限制

* **每日推荐不是一个真实歌单**：前端要像歌单一样点进详情页，所以 `NeteaseProvider::playlist("daily")`
  在 core 里合成一个 `id=daily` 的「假歌单」（封面取第一首、描述写「每天 6:00 更新」），
  这样详情页、播放全部、序号都能直接复用歌单页面，代价是它会被 `saved_playlists` 当普通歌单存。

* **会员曲仍需要账号**：`MUSIC_U` 有效但无 VIP 时，`fee=1` 的曲目取流仍是 `-110`，
  只能靠酷我桥接；两边都没有时如实报不可用。
* **`v3` 匿名也会给列表**：未登录时它返回的是通用推荐，不是「你的」。UI 因此把
  推荐页锁定在登录态之后。
* **匹配是启发式**：同名不同版本（Live/翻唱/伴奏）靠时长与版本词过滤，
  命中后 `via` 字段会写成 `kuwo-bridge/0.89`，便于排查。
* **协议会变**：`weapi` 是非公开接口，参数或魔数随时可能改；
  provider 里的所有端点都做了 `code` 判定与可读错误，不会静默降级成空列表。

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
- 静默检查：启动后 2.5 秒查一次、之后每 6 小时一次（`src/lib/update.svelte.ts`），
  只比版本号，不下载、不弹窗、失败当没发生；有新版本就在侧边栏「设置」右侧
  点一颗 6px 红点（`title` 里带版本号），点进设置才会看到完整信息。
  手动点「检查更新」确认已是最新时，红点顺手撤掉

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

网易云协议额外用到的只有三个纯 Rust 小 crate：`aes`（AES-128-CBC）、
`num-bigint`（RSA 模幂）、`md-5`（登录密码摘要），都不带 C 依赖。

### 本地构建注意（Windows + MinGW）

`src-tauri/Cargo.toml` 的 `[lib] crate-type` 只保留 `rlib`：

```toml
crate-type = ["rlib"]
```

`staticlib` / `cdylib` 是给 iOS/Android 用的。在 Windows + MinGW 下链接 cdylib 会因为
导出符号超过 65535 个直接失败（`ld: error: export ordinal too large`），
而出移动包时才需要它们。要出移动目标时再临时加回去。

MinGW 工具链需要出现在 `PATH` 里（本机在
`%LOCALAPPDATA%\tinymusic-toolchain\mingw64\bin`），否则 `libsqlite3-sys` 的
bundled 编译会报 `failed to find tool "gcc.exe"`。

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
切换会同时更新应用内配色和窗口外观（`set_window_theme`），系统主题变化时自动跟随。
macOS 上是 Overlay 标题栏（红绿灯的深浅由系统决定）；Windows 上标题栏是应用自己画的，
配色直接跟主题走，`set_window_theme` 只管系统那一层（边框、弹窗、原生控件）。

### 交互细节

- 所有 hover 都带 140–240ms 的过渡；不能插值的 `background-image` 渐变一律改写成
  可过渡的 `background-color`（卡片、歌单行、下载目标等），div / label 这类非按钮
  元素也单独补了 transition。
- 播放器的随机 / 循环 / 单曲循环打开时，按钮变成一小块状态玻璃，底下再亮一个 3px 圆点；
  不再是只换图标颜色那种看不出来的状态。
- 音量：点喇叭后一颗完整药丸从右端向左展开，滑块和喇叭都在药丸里，
  同时把歌词 / 队列两颗粒子按钮淡出并盖住；收起时反向播放同一段动画。
- 播放列表面板：最厚一档玻璃本来就是 96% 不透明，所以去掉了它背后的模糊——
  弹出 / 收起时不再每帧重算背景，动画回到 60fps。
- 打开播放列表时，正在播放的那首会自动滚到列表中间（面板开着时切歌也跟着滚），
  不用自己去找；关闭后 220ms 内忽略重新打开，快速连点不会出现「开了又关、关了又开」的闪烁。
- 设置 → 下载目录右侧的「选择…」直接调用系统的目录选择窗口（`tauri-plugin-dialog`），
  选完立即保存。
