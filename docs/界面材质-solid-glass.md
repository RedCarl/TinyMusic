# 界面材质：Solid Glass

界面的观感来自 [Solid Glass](https://github.com/deeplethe/solid-glass)（MIT，作者 DeepLethe）。
它原本是一份给 shadcn/ui 的主题样式表；TinyMusic 没有 React、Tailwind 和 shadcn，
所以这里移植的是它的**材质**，不是它的组件库：所有令牌、边缘和状态都落在
`src/app.css` 里，Svelte 组件的结构与类名保持原样。

一句话概括这套材质：远看是实心的，近看才是玻璃。透过的只有轮廓，不透过颜色，
也不打扰正在读的文字。

## 四条规则

1. **收边在边缘。** 顶上一道高光、四周一圈发丝线、底下一层阴影；面板正面保持中性，
   不铺渐变，不靠「亮一点 / 暗一点」的色块制造层次。
2. **厚重，不发飘。** 颜色是中性的，玻璃只把背后的东西柔化，不替它上色。
   深色页没有环境光，浅色页有三束很淡的光。
3. **深度即厚度。** 薄 → 中 → 厚：`--panel` / `--bg2` / `--popover`，
   越浮在上面越厚、越不透。
4. **只有一种选中态。** 悬停、选中、展开，都是同一小块透明玻璃：
   一层很淡的着色、顶上一道高光、四周一圈发丝线，没有阴影。
   第二种选中样式会让界面多出一套语言。

## 令牌

| 令牌 | 是什么 |
| --- | --- |
| `--panel` | 最薄的一层：铺版面、说明块、页签托盘 |
| `--bg2` | 中厚度：卡片、列表、歌单行 |
| `--popover` | 最厚：弹窗、右键菜单、队列、播放器栏 |
| `--glass-rim` | 顶边的高光 |
| `--glass-inner` | 高光里侧的第二道线，给边缘厚度 |
| `--glass-edge` | 切出页面的发丝线 |
| `--glass-shade` / `--glass-contact` / `--glass-lift` | 面板脚下、贴着页面、浮层再抬起的三层阴影 |
| `--glass-control` / `--glass-control-rim` | 一小块透明玻璃：次要按钮、标签、芯片 |
| `--glass-state` / `--glass-state-rim` / `--glass-state-armed` | 唯一的那一种状态，以及需要再亮一档时的选择态 |
| `--ink` / `--ink-fg` / `--ink-rim` / `--ink-under` | 墨色实心玻璃：主按钮、播放键、进度填充 |
| `--field` / `--field-rim` / `--field-edge` | 输入框：一块能写字的玻璃 |
| `--well-shade` / `--well-rim` | 凹槽：滑杆轨道、没勾的框，光从反方向落 |
| `--focus-line` | 键盘焦点的那条线（不是三像素宽的光圈） |
| `--radius` | 所有圆角从它推出来：`--r-sm/md/lg/xl` |
| `--brand` | 品牌绿。只在「有意义」的地方出现 |

阴影不是分散写的，而是四个合成令牌：`--pane-shadow`（面板）、`--float-shadow`
（浮层）、`--control-shadow`（小玻璃）、`--ink-shadow`（墨）、`--well-shadow`（凹槽）。
换一种厚度，换的是令牌，不是满页的 `box-shadow`。

颜色只留给有意义的东西：品牌绿（正在播放、已喜欢）、成功、失败、警告、
链接与云盘标记。其余全是墨与灰。

## 组件怎么落材质

| 界面元素 | 材质 |
| --- | --- |
| `aside` 侧边栏 | 不铺底、不描边：只排列按钮，落在窗口的统一表面上；分组只用一个 `.nav-sep` 的间距；宽度 204px |
| `main` 内容区 | 四周内缩 8px 的圆角矩形：`--panel` 薄底 + 一圈发丝线 + 脚下一层浅影；页面与它的色差很小，层次靠边缘而不是靠第二种底色 |
| `.list` / `.card` / `.plcard` / `.acct-card` / `.cap` / `.impq` | 中厚度面板（`--bg2` + `--pane-shadow`） |
| `.note` / `.cap` / `.impq` | 最薄的一层（`--panel` + `--pane-shadow-flat`） |
| `.modal` / `.ctx-menu` / `.queue-panel` / `.toast` / `.seek-tip` | 最厚（`--popover` + `--float-shadow`） |
| `.bar` 播放器栏 | 唯一的液态玻璃：`--player-bg`（24% / 55%）+ `--glass-blur-liquid`（模糊 + 增饱和 + 提亮）+ `--player-sheen`（左上镜面反光与顶边高光） |
| `.mini` / `.chips button` / `.actions button` / `.play` / `.badge` | 一小块透明玻璃（`--control-shadow`） |
| `.searchbar button` / `button.primary` / `.tb.main` / `.logo` / `.count` | 墨（`--ink-shadow`） |
| `.row:hover` / `.ctx-item:hover` / `.qp-row:hover` | `--hover-faint`，只上色、不描边 |
| `.row.current` / `.nav-ind` / `.tabs button.on` / `.s3row.on` / `.mode.on` | 唯一的那一种状态 |
| `.tb.on`（随机 / 循环 / 单曲循环） | 状态玻璃 + 一颗 3px 圆点：开关状态不再只靠图标颜色 |
| `.vol-slide`（音量展开） | 一颗不透明的药丸，从音量按钮向左长出，同时把被盖住的歌词 / 队列按钮淡出 |
| `.target.on` | 再亮一档的状态（`--glass-state-armed`）：这里必须一眼看出选了谁 |
| `input` / `select` / `textarea` | 输入框材质；聚焦只把线提亮 |
| `input[type=range]` / `.pbar-track` / `.vol-rail` / `.heat-bar` | 凹槽 + 墨 |
| `input[type=checkbox/radio]` | 未勾是凹槽，勾上是墨 |
| `.tabs` | 一个托盘（`--panel` + 凹槽），打开的页签是躺在里面的一块玻璃 |

## 唯一的例外：播放器栏

固态玻璃的四条规则里，「正面不铺光、几乎不透」对播放器栏不成立。
这一条栏要浮在正在滚动的内容上，用户需要看见背后在动，所以它是全站唯一一块
**液态玻璃**：

- 底色只留 24%（浅色 55%）的半透，背后靠 `blur(56px) saturate(220%) brightness(1.10)`
  去折射，而不是把内容挡死；
- 正面允许两层光：左上角一片很淡的镜面反光（`radial-gradient`）和顶边往下的一小段
  高光（`linear-gradient`），由 `--player-sheen` 一处给出；
- 顶边的高光比其它玻璃更亮（`--player-rim` 52%），脚下一层更深的阴影，让它仍然浮得起来；
- 形状是两端全圆的胶囊（`border-radius: 999px`，高 68px、左右内边距 20px），和 44px 的圆形播放键是同一套语言；进度条只在歌名下方、最长 400px，并让出右侧约 42px 给音量药丸；
- 文字与图标各压一层淡影（`text-shadow` / `drop-shadow`），透到浅色封面时也读得清；
- 系统要求减少透明时，`--player-bg` 换成实色、`--player-sheen` 取消，
  它退回成一块普通的实心板。

除此之外，界面里没有第二处液态玻璃 —— 弹窗、菜单、队列仍然是固态玻璃。

## 深色与浅色

深色是设计本身的形态（`:root`），浅色是同一套玻璃在天光下（`:root[data-theme='light']`）。
两套用**同一批名字**，所以组件里没有一处 `data-theme` 分支；浅色块里只剩几处
「不能靠颜色表达」的修正（遮罩浓度、封面占位色、自动填充底色）。

## 无障碍

- `prefers-reduced-transparency: reduce`：去掉模糊、玻璃换成实色，高光与阴影保留。
  也可以用 `data-glass="opaque"` 单独标一块区域。
- `prefers-reduced-motion: reduce`：所有动画与过渡关闭。动只表示「正在发生」
  （骨架、跳动的小竖线、进度扫光），结束或停下时不靠动画装饰。

## 想改的话

- **厚度/透明度**：改 `--panel`、`--bg2`、`--popover` 三个值（改一处，整页跟着变）。
- **圆角**：只改 `--radius`，其它圆角都是它的倍数。
- **对比**：`--glass-rim`、`--glass-edge`、`--glass-state*`；深色下它们比浅色更亮，
  因为深色页上没有环境光。
- **品牌色**：`--brand`、`--brand-fg`，两个主题各一份。
- **不要新增第二种选中态**：需要更明确时，用 `--glass-state-armed` 这一档，
  而不是另起一套颜色。

## 和上游的差异

上游靠 shadcn 的 `data-slot` 和 Tailwind 类名找组件，TinyMusic 用的是自己的语义类名
（`.row`、`.modal`、`.target`……）。因此：

- 所有规则写在 `src/app.css`，不引入 Tailwind 或任何运行时依赖；
- 「找组件」靠类名分组，不靠 `[data-slot]`；
- 阴影与合成写成 CSS 变量（`--pane-shadow` 等），组件只引用，不各写一份；
- 组件模板没有任何改动，改造只发生在样式层。
