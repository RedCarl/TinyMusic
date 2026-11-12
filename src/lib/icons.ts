/**
 * 统一图标出口。
 *
 * 规则（来自 hugeicons skill）：
 * * 全项目只用一套样式 —— 免费 Stroke Rounded（@hugeicons/core-free-icons）
 * * 同义同图标 —— 所有「搜索」都走 icons.search，不在组件里各写各的
 * * 图标名不许凭记忆拼 —— 这些名字都用 MCP search_icons 解析过，
 *   并用 `grep "declare const <Name>" node_modules/@hugeicons/core-free-icons/dist/types/index.d.ts` 校验存在
 * * 尺寸只用三档：16（行内操作）/ 18（导航与播放控制）/ 40（空状态）
 *
 * 侧边栏特别处理：`download` 用「下载目录」图标（文件夹+箭头），
 * `settings` 用齿轮 —— 两者形状差异明显，不会再看成同一个按钮；
 * 「喜欢」统一用爱心（HeartIcon）。
 */
import {
  Search01Icon,
  HeartIcon,
  Download01Icon,
  FolderDownloadIcon,
  Settings01Icon,
  MusicNote01Icon,
  PlayIcon,
  PauseIcon,
  PreviousIcon,
  NextIcon,
  VolumeHighIcon,
  VolumeIcon,
  VolumeLowIcon,
  VolumeOffIcon,
  ArrowLeft01Icon,
  ArrowRight01Icon,
  FolderOpenIcon,
  Playlist01Icon,
  UserAccountIcon,
  Login01Icon,
  PlusIcon,
  LinkIcon,
  ShuffleIcon,
  RepeatIcon,
  RepeatOne01Icon,
  ListMusicIcon,
  Queue01Icon,
  ChevronUpIcon,
  Cancel01Icon,
  CloudUploadIcon,
  HistoryIcon,
  TrashIcon,
  Loading03Icon,
  DiscIcon,
  QuoteIcon,
  Refresh01Icon,
  PencilEdit01Icon,
  DragDropVerticalIcon,
  FireIcon,
  MoreHorizontalIcon,
  HeadphonesIcon,
  PreferenceVerticalIcon,
  ShieldCheckIcon,
  Copy01Icon,
} from '@hugeicons/core-free-icons';

export const icons = {
  search: Search01Icon,
  /** 喜欢（爱心） */
  favorite: HeartIcon,
  /** 单曲下载 */
  download: Download01Icon,
  /** 侧边栏「下载」页：带箭头的文件夹，避免和齿轮图标混 */
  downloadDir: FolderDownloadIcon,
  settings: Settings01Icon,
  music: MusicNote01Icon,
  play: PlayIcon,
  pause: PauseIcon,
  previous: PreviousIcon,
  next: NextIcon,
  volume: VolumeIcon,
  volumeLow: VolumeLowIcon,
  volumeHigh: VolumeHighIcon,
  volumeOff: VolumeOffIcon,
  back: ArrowLeft01Icon,
  /** 向前：横向货架翻页、详情页的下一步 */
  forward: ArrowRight01Icon,
  folder: FolderOpenIcon,
  playlist: Playlist01Icon,
  account: UserAccountIcon,
  login: Login01Icon,
  plus: PlusIcon,
  link: LinkIcon,
  // ---- 播放列表 / 播放顺序 ----
  shuffle: ShuffleIcon,
  repeat: RepeatIcon,
  repeatOne: RepeatOne01Icon,
  list: ListMusicIcon,
  queue: Queue01Icon,
  chevronUp: ChevronUpIcon,
  close: Cancel01Icon,
  // ---- 网盘 / 历史 / 其他 ----
  cloud: CloudUploadIcon,
  history: HistoryIcon,
  trash: TrashIcon,
  loading: Loading03Icon,
  disc: DiscIcon,
  lyric: QuoteIcon,
  refresh: Refresh01Icon,
  edit: PencilEdit01Icon,
  grip: DragDropVerticalIcon,
  hot: FireIcon,
  more: MoreHorizontalIcon,
  quality: HeadphonesIcon,
  /** 均衡器：三根竖向推子，和音量 / 播放列表区分得开 */
  equalizer: PreferenceVerticalIcon,
  /** 网盘完整性检测 */
  shield: ShieldCheckIcon,
  /** 复制错误信息 */
  copy: Copy01Icon,
} as const;

/** 尺寸三档，避免各处随意取值 */
export const ICON_SIZE = {
  inline: 16,
  control: 18,
  empty: 40,
} as const;
