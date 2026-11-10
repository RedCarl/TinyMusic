import { invoke } from '@tauri-apps/api/core';
import { Channel } from '@tauri-apps/api/core';

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
}

/** 一次「歌单导入网盘」的结果 */
export interface S3TrackRow {
  key: string;
  track: Track;
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

export const api = {
  providers: () => invoke<ProviderInfo[]>('get_providers'),

  search: (provider: string, kw: string, page = 0, limit = 30) =>
    invoke<SearchPage>('search', { provider, kw, page, limit }),

  resolve: (track: Track, quality: Quality) =>
    invoke<{
      url: string;
      quality: Quality;
      requested: Quality;
      bytes: number | null;
      content_type: string;
      availability: Availability;
      format: AudioFormat;
      via: string;
      degraded: boolean;
      verified: boolean;
    }>('resolve', { track, quality }),

  qualityCapabilities: () => invoke<QualityCapability[]>('quality_capabilities'),

  qualitySet: (quality: Quality) => invoke<string>('quality_set', { quality }),

  trackQualities: (track: Track) => invoke<TrackQualities>('track_qualities', { track }),

  detail: (provider: string, track: Track) =>
    invoke<TrackDetail>('track_detail', { provider, track }),

  artist: (provider: string, artist_id: string, name: string, page = 0) =>
    invoke<ArtistPage>('artist', { provider, artistId: artist_id, name, page }),

  album: (provider: string, album_id: string, name: string, page = 0) =>
    invoke<AlbumPage>('album', { provider, albumId: album_id, name, page }),

  playerOpen: (track: Track, quality: Quality, forceRefresh = false) =>
    invoke<PlayerSource>('player_open', { track, quality, forceRefresh }),

  downloadStart: (track: Track, quality: Quality, onProgress: (p: Progress) => void) => {
    const ch = new Channel<Progress>();
    ch.onmessage = onProgress;
    return invoke<string>('download_start', { track, quality, onProgress: ch });
  },

  downloads: () => invoke<DownloadRow[]>('download_list'),
  downloadClear: () => invoke<void>('download_clear'),

  favoriteAdd: (track: Track) => invoke<void>('favorite_add', { track }),
  favoriteRemove: (provider: string, id: string) =>
    invoke<void>('favorite_remove', { provider, id }),
  favoriteList: () => invoke<Track[]>('favorite_list'),
  favoritesPull: () => invoke<Track[]>('favorites_pull'),
  favoriteCheck: (provider: string, id: string) =>
    invoke<boolean>('favorite_check', { provider, id }),

  searchPlaylists: (provider: string, kw: string, page = 0, limit = 30) =>
    invoke<PlaylistPage>('search_playlists', { provider, kw, page, limit }),

  playlist: (provider: string, id: string) =>
    invoke<PlaylistDetail>('playlist', { provider, id }),

  /** 按平台用户 id 列出歌单（酷我这条接口不需要登录） */
  userPlaylists: (provider: string, uid: string, limit = 100) =>
    invoke<PlaylistRef[]>('user_playlists', { provider, uid, limit }),

  /** 账号「喜欢」歌单 */
  favorites: (provider: string, uid: string) =>
    invoke<PlaylistDetail>('favorites', { provider, uid }),

  /** 批量下载（顺序执行） */
  downloadBatch: (
    tracks: Track[],
    quality: Quality,
    onProgress: (p: Progress) => void
  ) => {
    const ch = new Channel<Progress>();
    ch.onmessage = onProgress;
    return invoke<string[]>('download_batch', { tracks, quality, onProgress: ch });
  },

  /** 打开酷我网页登录窗口 */
  loginOpen: () => invoke<void>('login_open'),
  /** 探测是否已登录，返回 uid */
  loginVisible: () => invoke<boolean>('login_visible'),
  loginPurge: () => invoke<void>('login_purge'),
  setWindowTheme: (mode: 'auto' | 'light' | 'dark') =>
    invoke<void>('set_window_theme', { theme: mode === 'auto' ? null : mode }),
  loginProbe: () => invoke<string | null>('login_probe'),
  loginClose: () => invoke<void>('login_close'),

  savedPlaylistAdd: (playlist: PlaylistRef) =>
    invoke<void>('saved_playlist_add', { playlist }),
  savedPlaylistRemove: (provider: string, id: string) =>
    invoke<void>('saved_playlist_remove', { provider, id }),
  savedPlaylistList: () => invoke<PlaylistRef[]>('saved_playlist_list'),
  savedPlaylistCheck: (provider: string, id: string) =>
    invoke<boolean>('saved_playlist_check', { provider, id }),

  providerFeatures: (provider: string) =>
    invoke<{ favorites: boolean; recommend: boolean }>('provider_features', { provider }),

  authCapabilities: (provider: string) =>
    invoke<AuthCapability[]>('auth_capabilities', { provider }),
  login: (
    provider: string,
    method: AuthMethod,
    opts: { user?: string; password?: string; cookie?: string } = {}
  ) =>
    invoke<Session>('login', {
      provider,
      method,
      user: opts.user ?? null,
      password: opts.password ?? null,
      cookie: opts.cookie ?? null,
    }),
  logout: (provider: string) => invoke<void>('logout', { provider }),
  sessionGet: (provider: string) => invoke<Session | null>('session_get', { provider }),
  myPlaylists: (provider: string) => invoke<PlaylistRef[]>('my_playlists', { provider }),

  // ---- 网易云（目录侧）----
  /** 每日推荐（需要登录态） */
  recommendSongs: (provider: string, limit = 30) =>
    invoke<Track[]>('recommend_songs', { provider, limit }),
  /** 推荐歌单 */
  recommendPlaylists: (provider: string) =>
    invoke<PlaylistRef[]>('recommend_playlists', { provider }),
  /**
   * 打开网易云网页登录窗口。
   *
   * 登录结果通过事件回传：
   * - `netease://login`       登录成功（payload = Session）
   * - `netease://login_error` 拿到 cookie 但校验失败
   * - `netease://login_timeout` 超时未登录
   */
  neteaseLoginOpen: () => invoke<void>('netease_login_open'),

  settingsGet: () => invoke<Settings>('settings_get'),
  settingsSet: (settings: Settings) => invoke<void>('settings_set', { settings }),
  openDownloadDir: () => invoke<void>('open_download_dir'),

  // ---- 网盘（S3） ----
  s3Test: () => invoke<string>('s3_test'),
  s3Playlists: () => invoke<S3PlaylistInfo[]>('s3_playlists'),
  s3PlaylistCreate: (name: string) => invoke<string>('s3_playlist_create', { name }),
  s3PlaylistDelete: (name: string) => invoke<void>('s3_playlist_purge', { name }),
  s3PlaylistRename: (oldName: string, newName: string) =>
    invoke<string>('s3_playlist_rename', { old: oldName, new: newName }),
  s3PlaylistTracks: (name: string) => invoke<S3TrackRow[]>('s3_playlist_tracks', { name }),
  s3ReorderPlaylists: (names: string[]) => invoke<void>('s3_playlists_reorder', { names }),
  s3TrackDelete: (key: string) => invoke<void>('s3_track_delete', { key }),
  s3Download: (
    track: Track,
    quality: Quality,
    playlist: string,
    onProgress: (p: Progress) => void
  ) => {
    const ch = new Channel<Progress>();
    ch.onmessage = onProgress;
    return invoke<string>('s3_download', { track, quality, playlist, onProgress: ch });
  },
  s3DownloadBatch: (
    tracks: Track[],
    quality: Quality,
    playlist: string,
    onProgress: (p: Progress) => void
  ) => {
    const ch = new Channel<Progress>();
    ch.onmessage = onProgress;
    return invoke<string[]>('s3_download_batch', { tracks, quality, playlist, onProgress: ch });
  },

  /** 把一个平台歌单整张导入到网盘（后台逐首下载 + 上传） */
  s3ImportPlaylist: (
    provider: string,
    playlistId: string,
    name: string,
    target: string,
    onProgress: (p: Progress) => void
  ) => {
    const ch = new Channel<Progress>();
    ch.onmessage = onProgress;
    return invoke<S3ImportResult>('s3_import_playlist', {
      provider,
      playlistId,
      name,
      target,
      onProgress: ch,
    });
  },

  /** 把主窗口收进托盘（播放不中断） */
  hideToTray: () => invoke<void>('hide_to_tray'),
};

export function fmtDuration(sec: number): string {
  if (!sec || sec < 0) return '';
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${String(s).padStart(2, '0')}`;
}

export function fmtBytes(n: number | null | undefined): string {
  if (!n) return '';
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

/** 音质代码 → 中文，下载记录等处直接展示 */
export const QUALITY_LABEL: Record<string, string> = {
  standard: '标准 128k',
  high: '标准 128k', // 旧记录兼容
  super: '高品质 320k',
  lossless: '无损 FLAC',
  hires: 'Hi-Res',
  atmos: '杜比全景声',
  master: '母带',
  original: '原文件',
};

export function qualityLabel(q: string): string {
  return QUALITY_LABEL[q] ?? q;
}

/** 实际格式 → 界面短标签，例如 `FLAC 44.1kHz/16bit`、`MP3 320k` */
export function formatLabel(f: AudioFormat | null | undefined): string {
  if (!f || f.codec === 'unknown') return '';
  const codec = f.codec.toUpperCase();
  if (f.lossless) {
    const hz = f.sample_rate_hz ? `${(f.sample_rate_hz / 1000).toFixed(1).replace('.0', '')}kHz` : '';
    const bits = f.bits_per_sample ? `${f.bits_per_sample}bit` : '';
    return [codec, hz, bits].filter(Boolean).join(' ');
  }
  return f.bitrate_kbps ? `${codec} ${f.bitrate_kbps}k` : codec;
}

export const AVAILABILITY_LABEL: Record<Availability, string> = {
  unknown: '',
  available: '',
  preview_only: '仅试听',
  vip_only: '会员曲',
  unavailable: '不可用',
};

/** 该曲目是否确定无法播放。
 *
 * 注意：`vip_only`（PAY=16711935）**不再**算作不可播放 ——
 * provider 现在会落到资源管理链路拿到真直链，会员曲可以正常播放，
 * 该标记只作为「这是会员内容」的提示。 */
export function isBlocked(a: Availability): boolean {
  return a === 'unavailable';
}


/** 从酷我歌单链接 / 纯数字 / playlist?id= 中抽出 pid */
export function parsePlaylistId(input: string): string | null {
  const t = input.trim();
  if (!t) return null;
  if (/^\d+$/.test(t)) return t;
  const m =
    t.match(/playlist[_-]?detail\/(\d+)/i) ||
    t.match(/[?&](?:id|pid|playlistid)=(\d+)/i) ||
    t.match(/(\d{6,})/);
  return m ? m[1] : null;
}

/**
 * 酷我封面 URL 自带尺寸段（如 `/star/albumcover/120/...`），
 * 把尺寸段换成更大的值就能拿到同图的高清版（实测 500/800/1000 都可用）。
 * 列表缩略图继续用 120，详情/歌词这种大图再用这个函数换大图。
 */
export function coverUrl(url: string | null | undefined, size = 500): string {
  if (!url) return '';
  if (!/kuwo\.cn/i.test(url)) return url;
  return url.replace(/\/(\d{2,4})\//, `/${size}/`);
}

export function fmtCount(n: number): string {
  if (!n) return '';
  if (n >= 100_000_000) return `${(n / 100_000_000).toFixed(1)}亿`;
  if (n >= 10_000) return `${(n / 10_000).toFixed(1)}万`;
  return String(n);
}
