/**
 * 曲目 / 专辑的右键菜单构造器。
 *
 * 菜单项本身只描述「做什么」，渲染统一交给 <ContextMenu />。
 */
import { api, isBlocked, type AlbumRef, type Track } from './api';
import { icons } from './icons';
import { openMenuAt, type MenuEntry } from './contextmenu.svelte';
import { playTrack, playNext, addToQueue, inQueue } from './player.svelte';
import { requestDownload } from './download.svelte';
import { go } from './router.svelte';
import { appCfg } from './config.svelte';
import { showToast } from './toast.svelte';

export interface TrackMenuOptions {
  /** 从哪个列表点出来的，播放时把整个列表作为队列 */
  queue?: Track[];
  onFavorite?: (fav: boolean) => void;
  /** 额外插入到末尾的菜单项（如播放列表里的「从列表移除」） */
  extra?: MenuEntry[];
}

/** 歌曲右键菜单：播放 / 下一首 / 加入列表 / 下载 / 喜欢 / 详情 / 歌手 / 专辑 */
export async function openTrackMenu(
  e: MouseEvent,
  track: Track,
  opts: TrackMenuOptions = {}
) {
  e.preventDefault();
  e.stopPropagation();
  const x = e.clientX;
  const y = e.clientY;

  const blocked = isBlocked(track.availability);
  let fav = false;
  // 本地 SQLite 查一下收藏状态，决定菜单写「喜欢」还是「取消喜欢」
  try {
    fav = await api.favoriteCheck(track.provider, track.id);
  } catch {
    /* 查不到就按未收藏处理 */
  }

  const items: MenuEntry[] = [
    {
      label: '播放',
      icon: icons.play,
      disabled: blocked,
      run: () => {
        void playTrack(track, opts.queue);
      },
    },
    {
      label: '下一首播放',
      icon: icons.next,
      run: () => {
        playNext(track);
        showToast('已设为下一首');
      },
    },
    {
      label: inQueue(track) ? '已在播放列表' : '添加到播放列表',
      icon: icons.queue,
      disabled: inQueue(track),
      run: () => {
        if (addToQueue(track)) showToast('已加入播放列表');
      },
    },
    { sep: true },
    {
      label: '下载（含歌词与封面）',
      icon: icons.download,
      disabled: blocked || track.provider === 's3',
      run: () => {
        void requestDownload([track]);
      },
    },
    ...(appCfg.s3Enabled
      ? [
          {
            label: fav ? '取消喜欢' : '喜欢（收藏到网盘）',
            icon: icons.favorite,
            run: async () => {
              if (fav) {
                await api.favoriteRemove(track.provider, track.id);
                opts.onFavorite?.(false);
                showToast('已取消喜欢');
              } else {
                await api.favoriteAdd(track);
                opts.onFavorite?.(true);
                showToast('已收藏到网盘');
              }
            },
          },
        ]
      : []),
    { sep: true },
    {
      label: '查看歌曲详情',
      icon: icons.music,
      run: () => go({ name: 'track', track }),
    },
    ...(track.artist_id
      ? [
          {
            label: '打开歌手',
            icon: icons.account,
            run: () =>
              go({
                name: 'artist' as const,
                provider: track.provider,
                artistId: track.artist_id,
                title: track.artist,
              }),
          },
        ]
      : []),
    ...(track.album_id
      ? [
          {
            label: '打开专辑',
            icon: icons.disc,
            run: () =>
              go({
                name: 'album' as const,
                provider: track.provider,
                albumId: track.album_id,
                title: track.album,
              }),
          },
        ]
      : []),
    ...(opts.extra ?? []),
  ];

  openMenuAt(x, y, items);
}

/** 专辑卡片右键：打开 / 播放全部 / 下载全部（需要时再拉专辑曲目） */
export function openAlbumMenu(e: MouseEvent, album: AlbumRef, provider = 'kuwo') {
  e.preventDefault();
  e.stopPropagation();
  const x = e.clientX;
  const y = e.clientY;

  const fetchTracks = () => api.album(provider, album.id, album.name).then((d) => d.tracks);

  openMenuAt(x, y, [
    {
      label: '打开专辑',
      icon: icons.disc,
      run: () =>
        go({ name: 'album', provider, albumId: album.id, title: album.name }),
    },
    {
      label: '播放全部',
      icon: icons.play,
      run: async () => {
        const tracks = await fetchTracks();
        if (tracks.length) await playTrack(tracks[0], tracks);
      },
    },
    {
      label: '下载全部',
      icon: icons.download,
      run: async () => {
        const tracks = await fetchTracks();
        if (tracks.length) void requestDownload(tracks);
      },
    },
  ]);
}

/** 一组曲目的通用菜单（专辑页/歌单页头部用） */
export function openTracksMenu(e: MouseEvent, tracks: Track[]) {
  e.preventDefault();
  e.stopPropagation();
  openMenuAt(e.clientX, e.clientY, [
    {
      label: '播放全部',
      icon: icons.play,
      disabled: tracks.length === 0,
      run: () => {
        if (tracks.length) void playTrack(tracks[0], tracks);
      },
    },
    {
      label: '下载全部',
      icon: icons.download,
      disabled: tracks.length === 0,
      run: () => {
        if (tracks.length) void requestDownload(tracks);
      },
    },
  ]);
}
