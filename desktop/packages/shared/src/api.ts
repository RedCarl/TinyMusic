//! 后端命令的 TS 门面：desktop 与 mobile 完全共用。
//!
//! 只描述「调什么命令、参数叫什么」；怎么送出去由 transport 决定。

import type { ProgressCallback } from './transport';
import { invoke, invokeTask, shellInvoke } from './transport';
import type { Availability, Quality, Track, Codec, AudioFormat, ProviderInfo, QualityCapability, TrackQualities, SearchPage, LyricLine, TrackDetail, AlbumRef, ArtistPage, AlbumPage, PlayerSource, DownloadRow, FavoriteDuplicate, S3IntegrityIssue, S3IntegrityReport, S3FixProgress, FailedPartName, FailedPart, FailedDownload, Settings, S3PlaylistInfo, S3TrackRow, S3ImportResult, PlaylistRef, PlaylistPage, PlaylistDetail, AuthMethod, AuthCapability, Session, Progress } from './types';

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

  playerOpen: (track: Track, quality: Quality, forceRefresh = false, allowLocal = true) =>
    invoke<PlayerSource>('player_open', { track, quality, forceRefresh, allowLocal }),

  downloadStart: (track: Track, quality: Quality, onProgress: (p: Progress) => void) => {
    return invokeTask<string>('download_start', { track, quality }, onProgress as ProgressCallback);
  },

  downloads: () => invoke<DownloadRow[]>('download_list'),
  downloadClear: () => invoke<void>('download_clear'),

  // ---- 下载失败记录 / 手动重试 ----
  downloadFailures: () => invoke<FailedDownload[]>('download_failures'),
  /** 只补失败的部分；返回重试后仍然失败的记录 */
  downloadRetryFailed: (ids: string[]) =>
    invoke<FailedDownload[]>('download_retry_failed', { ids }),
  downloadFailureDismiss: (id: string) =>
    invoke<void>('download_failure_dismiss', { id }),
  downloadFailuresClear: () => invoke<void>('download_failures_clear'),

  favoriteAdd: (track: Track) => invoke<void>('favorite_add', { track }),
  favoriteRemove: (track: Track) =>
    invoke<void>('favorite_remove', {
      provider: track.provider,
      id: track.id,
      title: track.title,
      artist: track.artist,
    }),
  favoriteList: () => invoke<Track[]>('favorite_list'),
  favoritesPull: () => invoke<Track[]>('favorites_pull'),
  /** 扫「喜欢」目录里跟别的歌单重复的副本（桌面端维护入口） */
  favoritesDedupeScan: () => invoke<FavoriteDuplicate[]>('favorites_dedupe_scan'),
  /** 删掉选中的重复副本，返回释放的字节数 */
  favoritesDedupePurge: (keys: string[]) =>
    invoke<number>('favorites_dedupe_purge', { keys }),
  /**
   * 是否已收藏。
   *
   * 收藏存在网盘「喜欢」目录里，id 是那边的副本 key，跟歌单里的原始 key 不同，
   * 所以要把标题/歌手一起带上，后端用它认「同一首歌」。
   */
  favoriteCheck: (track: Track) =>
    invoke<boolean>('favorite_check', {
      provider: track.provider,
      id: track.id,
      title: track.title,
      artist: track.artist,
    }),

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
    return invokeTask<string[]>('download_batch', { tracks, quality }, onProgress as ProgressCallback);
  },

  /** 打开酷我网页登录窗口 */
  loginOpen: () => shellInvoke<void>('login_open'),
  /** 探测是否已登录，返回 uid */
  loginVisible: () => shellInvoke<boolean>('login_visible'),
  loginPurge: () => shellInvoke<void>('login_purge'),
  setWindowTheme: (mode: 'auto' | 'light' | 'dark') =>
    shellInvoke<void>('set_window_theme', { theme: mode === 'auto' ? null : mode }),
  loginProbe: () => shellInvoke<string | null>('login_probe'),
  loginClose: () => shellInvoke<void>('login_close'),

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
  neteaseLoginOpen: () => shellInvoke<void>('netease_login_open'),

  settingsGet: () => invoke<Settings>('settings_get'),
  settingsSet: (settings: Settings) => invoke<void>('settings_set', { settings }),
  openDownloadDir: () => shellInvoke<void>('open_download_dir'),

  // ---- 网盘（S3） ----
  s3Test: () => invoke<string>('s3_test'),
  s3Playlists: () => invoke<S3PlaylistInfo[]>('s3_playlists'),
  s3PlaylistCreate: (name: string) => invoke<string>('s3_playlist_create', { name }),
  s3PlaylistDelete: (name: string) => invoke<void>('s3_playlist_purge', { name }),
  s3PlaylistRename: (oldName: string, newName: string) =>
    invoke<string>('s3_playlist_rename', { old: oldName, new: newName }),
  s3PlaylistTracks: (name: string, root = false) =>
    invoke<S3TrackRow[]>('s3_playlist_tracks', { name, root }),
  s3ReorderPlaylists: (names: string[]) => invoke<void>('s3_playlists_reorder', { names }),
  /** 重新签一个网盘曲目的封面直链（签名地址会过期） */
  s3Cover: (key: string) => invoke<string | null>('s3_cover', { key }),

  // ---- 网盘完整性检测 ----
  /** 检查整库（playlist 为空）或某个歌单里缺音频 / 歌词 / 封面的条目 */
  s3IntegrityCheck: (playlist?: string | null) =>
    invoke<S3IntegrityReport>('s3_integrity_check', { playlist: playlist || null }),
  /** 补齐选中的条目，返回补齐后重新扫描的结果 */
  s3IntegrityFix: (
    playlist: string | null,
    ids: string[],
    onProgress: (p: S3FixProgress) => void
  ) => {
    return invokeTask<S3IntegrityReport>('s3_integrity_fix', { playlist,
      ids
    }, onProgress as ProgressCallback);
  },
  s3TrackDelete: (key: string) => invoke<void>('s3_track_delete', { key }),
  s3Download: (
    track: Track,
    quality: Quality,
    playlist: string,
    onProgress: (p: Progress) => void
  ) => {
    return invokeTask<string>('s3_download', { track, quality, playlist }, onProgress as ProgressCallback);
  },
  s3DownloadBatch: (
    tracks: Track[],
    quality: Quality,
    playlist: string,
    onProgress: (p: Progress) => void
  ) => {
    return invokeTask<string[]>('s3_download_batch', { tracks, quality, playlist }, onProgress as ProgressCallback);
  },

  /** 把一个平台歌单整张导入到网盘（后台逐首下载 + 上传） */
  s3ImportPlaylist: (
    provider: string,
    playlistId: string,
    name: string,
    target: string,
    onProgress: (p: Progress) => void
  ) => {
    return invokeTask<S3ImportResult>('s3_import_playlist', { provider,
      playlistId,
      name,
      target
    }, onProgress as ProgressCallback);
  },

  /** 把主窗口收进托盘（播放不中断） */
  hideToTray: () => shellInvoke<void>('hide_to_tray'),
};
