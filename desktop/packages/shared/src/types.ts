//! 后端 JSON 契约的 TS 镜像（字段名与 Rust serde 输出一致，**前后端共用**）。
//!
//! 改这里就等于改契约：desktop 与 mobile 会同时受影响，所以别在两端各自复制。

export type Availability = 'unknown' | 'available' | 'preview_only' | 'vip_only' | 'unavailable';
export type Quality =
  | 'standard'
  | 'super'
  | 'lossless'
  | 'hires'
  | 'atmos'
  | 'master';

export interface Track {
  provider: string;
  id: string;
  title: string;
  artist: string;
  artist_id: string;
  album: string;
  album_id: string;
  duration_sec: number;
  cover: string | null;
  availability: Availability;
  subtitle: string | null;
  /** 列表接口 MINFO 解析出的可播档位；空表示未知 */
  available_qualities?: Quality[];
  /** 目录里存在加密高级音质（当前不可播） */
  has_advanced?: boolean;
}

export type Codec = 'mp3' | 'aac' | 'flac' | 'alac' | 'ogg' | 'wav' | 'unknown';

export interface AudioFormat {
  codec: Codec;
  bitrate_kbps: number | null;
  sample_rate_hz: number | null;
  bits_per_sample: number | null;
  channels: number | null;
  lossless: boolean;
  verified: boolean;
}

export interface ProviderInfo {
  id: string;
  name: string;
  qualities: Quality[];
}

export interface QualityCapability {
  value: Quality;
  label: string;
  kbps: number | null;
  lossless: boolean;
  available: boolean;
  reason: string | null;
}

export interface TrackQualities {
  available: Quality[];
  has_advanced: boolean;
  known: boolean;
}

export interface SearchPage {
  total: number;
  page: number;
  limit: number;
  items: Track[];
}

export interface LyricLine {
  time_ms: number;
  text: string;
}

export interface TrackDetail {
  track: Track;
  lyrics: LyricLine[];
  preview_range_ms: [number, number] | null;
}

export interface AlbumRef {
  id: string;
  name: string;
  cover: string | null;
  year: string | null;
}

export interface ArtistPage {
  artist_id: string;
  name: string;
  pic: string | null;
  desc: string | null;
  meta: [string, string][];
  albums: AlbumRef[];
  tracks: Track[];
}

export interface AlbumPage {
  album_id: string;
  name: string;
  artist: string;
  cover: string | null;
  tracks: Track[];
}

export interface PlayerSource {
  url: string;
  /** 实际档位；网盘文件为 original */
  quality: string;
  requested_quality: string;
  format: AudioFormat;
  bytes: number | null;
  degraded: boolean;
  verified: boolean;
  via: string;
}

export interface DownloadRow {
  task_id: string;
  provider: string;
  id: string;
  title: string;
  artist: string;
  path: string;
  quality: string;
  bytes: number;
  status: string;
  created_at: number;
}

/** 网盘完整性检测结果：一首歌缺了哪些部分 */
export interface S3IntegrityIssue {
  /** 稳定 id：歌单 + 文件主干 */
  id: string;
  /** 歌单名（prefix 下的目录） */
  playlist: string;
  /** 音频 key 去掉扩展名后的路径 */
  stem: string;
  /** 现存音频 key；音频缺失时为空 */
  key: string;
  title: string;
  artist: string;
  /** audio / lyrics / cover */
  missing: FailedPartName[];
  /** 能补齐的来源平台（kuwo / netease），空表示找不到来源 */
  source: string;
  can_fix: boolean;
}

/** 「喜欢」目录里跟别的歌单重复的副本（桌面端手动清理用） */
export interface FavoriteDuplicate {
  /** 保留的那份：歌单里的原文件 */
  canonical: string;
  /** 「喜欢」目录里可以删的那份 */
  duplicate: string;
  bytes: number;
}

export interface S3IntegrityReport {
  checked: number;
  issues: S3IntegrityIssue[];
}

/** 补齐过程中逐条回传的进度 */
export interface S3FixProgress {
  id: string;
  status: 'running' | 'done' | 'failed' | (string & {});
  /** 这次之后仍然缺的部分；done 时为空 */
  missing: FailedPartName[];
  error: string;
  /** 已处理数量；running 时是当前项的序号 */
  done: number;
  total: number;
}

/** 下载里失败的一个部分 */
export type FailedPartName = 'audio' | 'lyrics' | 'cover' | (string & {});

export interface FailedPart {
  part: FailedPartName;
  /** 最后一次尝试的错误，给用户看原因用 */
  error: string;
}

/**
 * 一条「下载不完整」记录（音频 / 歌词 / 封面里哪部分失败）。
 * 自动重试 5 次后仍失败才会出现；手动重试成功后后端会删掉这条。
 */
export interface FailedDownload {
  id: string;
  track: Track;
  quality: Quality;
  /** local：下载到本地；s3：上传到网盘 */
  target: 'local' | 's3' | (string & {});
  /** 网盘歌单名（本地下载为空） */
  playlist: string;
  /** 本地文件路径或网盘对象 key；音频都没下成功时为空 */
  location: string;
  parts: FailedPart[];
  created_at: number;
}

export interface Settings {
  download_dir: string;
  default_quality: Quality;
  settings_version: number;
  concurrency: number;
  // ---- 网盘（S3 兼容） ----
  s3_enabled: boolean;
  s3_endpoint: string;
  s3_region: string;
  s3_bucket: string;
  s3_access_key: string;
  s3_secret_key: string;
  s3_prefix: string;
  s3_path_style: boolean;
  // ---- 均衡器 ----
  eq_enabled: boolean;
  /** 10 段增益（dB） */
  eq_gains: number[];
  eq_preamp: number;
  // ---- 托盘 ----
  close_to_tray: boolean;
  // ---- 网易云 ----
  /**
   * 播放策略：
   * - `netease_first`（默认）：先试网易云直链，失败回落酷我
   * - `kuwo_only`：网易云只当目录，音源一律走酷我
   */
  netease_play_mode: 'netease_first' | 'kuwo_only';
}

/** 网盘里的歌单（= 桶内按目录归类） */
export interface S3PlaylistInfo {
  name: string;
  count: number;
  cover?: string | null;
  /** true 表示根目录散歌的伪歌单（「未分类」） */
  root?: boolean;
}

/** 一次「歌单导入网盘」的结果 */
export interface S3TrackRow {
  key: string;
  track: Track;
  /** 列表里顺手标出的缺失项：lyrics / cover */
  missing?: string[];
}

export interface S3ImportResult {
  playlist: string;
  total: number;
  uploaded: number;
  failed: number;
}

export interface PlaylistRef {
  provider: string;
  id: string;
  name: string;
  cover: string | null;
  track_count: number;
  creator: string | null;
  description: string | null;
  play_count: number;
  tags: string | null;
}

export interface PlaylistPage {
  total: number;
  page: number;
  items: PlaylistRef[];
}

export interface PlaylistDetail {
  playlist: PlaylistRef;
  tracks: Track[];
}

export type AuthMethod = 'password' | 'cookie' | 'qr_code';

export interface AuthCapability {
  method: AuthMethod;
  supported: boolean;
  reason: string | null;
}

export interface Session {
  provider: string;
  user_id: string;
  nickname: string;
  avatar: string | null;
  cookies: [string, string][];
  extra: Record<string, string>;
  created_at: number;
}

export interface Progress {
  task_id: string;
  downloaded: number;
  total: number | null;
}
