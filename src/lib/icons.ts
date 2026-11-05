/**
 * 统一图标出口。
 *
 * 规则（来自 hugeicons skill）：
 * * 全项目只用一套样式 —— 免费 Stroke Rounded（@hugeicons/core-free-icons）
 * * 同义同图标 —— 所有「搜索」都走 icons.search，不在组件里各写各的
 * * 图标名不许凭记忆拼 —— 这些名字都用 MCP search_icons 解析过，
 *   并用 `grep "declare const <Name>" node_modules/@hugeicons/core-free-icons/dist/types/index.d.ts` 校验存在
 * * 尺寸只用三档：16（行内操作）/ 18（导航与播放控制）/ 40（空状态）
 */
import {
  Search01Icon,
  StarIcon,
  Download01Icon,
  Settings01Icon,
  MusicNote01Icon,
  PlayIcon,
  PauseIcon,
  PreviousIcon,
  NextIcon,
  VolumeIcon,
  ArrowLeft01Icon,
  FolderOpenIcon,
  Playlist01Icon,
  UserAccountIcon,
  Login01Icon,
  PlusIcon,
  LinkIcon,
} from '@hugeicons/core-free-icons';

export const icons = {
  search: Search01Icon,
  favorite: StarIcon,
  download: Download01Icon,
  settings: Settings01Icon,
  music: MusicNote01Icon,
  play: PlayIcon,
  pause: PauseIcon,
  previous: PreviousIcon,
  next: NextIcon,
  volume: VolumeIcon,
  back: ArrowLeft01Icon,
  folder: FolderOpenIcon,
  playlist: Playlist01Icon,
  account: UserAccountIcon,
  login: Login01Icon,
  plus: PlusIcon,
  link: LinkIcon,
} as const;

/** 尺寸三档，避免各处随意取值 */
export const ICON_SIZE = {
  inline: 16,
  control: 18,
  empty: 40,
} as const;
