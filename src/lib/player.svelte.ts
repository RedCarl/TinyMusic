import { api, type Quality, type Track } from './api';

/** 播放器全局状态（Svelte 5 runes） */
export const player = $state({
  current: null as Track | null,
  queue: [] as Track[],
  index: -1,
  playing: false,
  loading: false,
  src: '',
  progress: 0, // 秒
  duration: 0, // 秒
  volume: 1,
  quality: ((typeof localStorage !== 'undefined' && localStorage.getItem('quality')) ||
    'high') as Quality,
  error: '' as string,
});

/** 音质选项（顺序即 UI 顺序） */
export const QUALITY_OPTIONS: { value: Quality; label: string }[] = [
  { value: 'standard', label: '标准' },
  { value: 'high', label: '高品' },
  { value: 'super', label: '超高' },
  { value: 'lossless', label: '无损' },
];

/** 切换音质：持久化 + 若正在播放则按新音质重新取流（保持进度） */
export async function setQuality(q: Quality) {
  player.quality = q;
  localStorage.setItem('quality', q);
  const t = player.current;
  if (!t) return;
  const at = player.progress;
  const wasPlaying = player.playing;
  try {
    player.loading = true;
    const src = await api.playerOpen(t.provider, t.id, q);
    player.src = src.url;
    listener?.('load', undefined);
    // 恢复播放位置：等 metadata 就绪后再 seek
    setTimeout(() => {
      listener?.('seek', at);
      if (wasPlaying) {
        listener?.('play', undefined);
        player.playing = true;
      }
    }, 260);
    player.error = '';
  } catch (e) {
    player.error = String(e);
  } finally {
    player.loading = false;
  }
}

type Listener = (action: 'load' | 'play' | 'pause' | 'seek', value?: number) => void;
let listener: Listener | null = null;

/** App.svelte 注册音频元素的桥接 */
export function bindAudio(fn: Listener) {
  listener = fn;
}

export async function playTrack(track: Track, queue?: Track[]) {
  player.error = '';
  if (queue && queue.length) {
    player.queue = queue;
    player.index = queue.findIndex((t) => t.id === track.id && t.provider === track.provider);
  } else if (!player.queue.some((t) => t.id === track.id && t.provider === track.provider)) {
    player.queue = [track];
    player.index = 0;
  } else {
    player.index = player.queue.findIndex(
      (t) => t.id === track.id && t.provider === track.provider
    );
  }

  player.current = track;
  player.loading = true;
  player.progress = 0;
  player.duration = track.duration_sec || 0;

  try {
    const src = await api.playerOpen(track.provider, track.id, player.quality);
    player.src = src.url;
    player.loading = false;
    listener?.('load', undefined);
    // load 后再 play，避免 WebView 上偶发的播放被忽略
    setTimeout(() => {
      listener?.('play', undefined);
      player.playing = true;
    }, 30);
  } catch (e) {
    player.loading = false;
    const msg = String(e);
    player.error = msg.includes('unavailable')
      ? '这首曲目在各音质档位都没取到完整音频'
      : msg;
    player.playing = false;
    // 在列表里标记为不可用
    const t = player.queue.find((x) => x.id === track.id);
    if (t) t.availability = 'unavailable';
  }
}

export function toggle() {
  if (!player.current) return;
  if (player.playing) {
    listener?.('pause', undefined);
    player.playing = false;
  } else {
    listener?.('play', undefined);
    player.playing = true;
  }
}

export function seek(sec: number) {
  listener?.('seek', sec);
  player.progress = sec;
}

export function setVolume(v: number) {
  player.volume = v;
  localStorage.setItem('volume', String(v));
}

export function next() {
  if (player.queue.length === 0) return;
  const i = (player.index + 1) % player.queue.length;
  player.index = i;
  playTrack(player.queue[i]);
}

export function prev() {
  if (player.queue.length === 0) return;
  const i = (player.index - 1 + player.queue.length) % player.queue.length;
  player.index = i;
  playTrack(player.queue[i]);
}

export function onEnded() {
  if (player.queue.length > 1) next();
  else player.playing = false;
}

export function restoreVolume() {
  const v = Number(localStorage.getItem('volume'));
  if (!Number.isNaN(v) && v >= 0 && v <= 1) player.volume = v;
}
