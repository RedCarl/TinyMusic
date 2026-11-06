import { api, type LyricLine, type Quality, type Track } from './api';

/** 播放状态：播放按钮据此显示「播放 / 暂停 / 继续播放 / 加载中」 */
export type PlayStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'ended' | 'error';

/** 播放顺序 */
export type PlayMode = 'order' | 'repeat_all' | 'repeat_one' | 'shuffle';

export interface PlayModeMeta {
  value: PlayMode;
  label: string;
  hint: string;
  /** 与 icons.ts 中的键对应，组件里再映射成图标组件 */
  iconKey: 'list' | 'repeat' | 'repeatOne' | 'shuffle';
}

export const PLAY_MODES: PlayModeMeta[] = [
  { value: 'order', label: '顺序播放', hint: '按列表顺序播放，播完最后一首停止', iconKey: 'list' },
  { value: 'repeat_all', label: '列表循环', hint: '播完最后一首后回到第一首', iconKey: 'repeat' },
  { value: 'repeat_one', label: '单曲循环', hint: '一直重复当前这一首', iconKey: 'repeatOne' },
  { value: 'shuffle', label: '随机播放', hint: '随机挑选下一首', iconKey: 'shuffle' },
];

/** 音质选项（顺序即 UI 顺序） */
export const QUALITY_OPTIONS: { value: Quality; label: string }[] = [
  { value: 'standard', label: '标准' },
  { value: 'high', label: '高品' },
  { value: 'super', label: '超高' },
];

/** 无损档实测拿不到完整音频，已从产品里移除；旧配置里残留下就退回超高 */
function normalizeQuality(raw: string): Quality {
  if (raw === 'lossless') return 'super';
  if (raw === 'standard' || raw === 'high' || raw === 'super') return raw;
  return 'high';
}

function readLS(key: string): string {
  try {
    return localStorage.getItem(key) ?? '';
  } catch {
    return '';
  }
}

function writeLS(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* 隐私模式忽略 */
  }
}

function readMode(): PlayMode {
  const v = readLS('play_mode');
  return v === 'repeat_all' || v === 'repeat_one' || v === 'shuffle' ? v : 'order';
}

/** 播放器全局状态（Svelte 5 runes） */
export const player = $state({
  current: null as Track | null,
  queue: [] as Track[],
  index: -1,
  playing: false,
  loading: false,
  status: 'idle' as PlayStatus,
  src: '',
  progress: 0, // 秒
  duration: 0, // 秒
  volume: 1,
  quality: normalizeQuality(readLS('quality')) as Quality,
  error: '' as string,
  mode: readMode() as PlayMode,
  /** 播放列表弹窗是否展开 */
  queueOpen: false,
  /** 音量面板是否展开 */
  volumeOpen: false,
  /** 静音前的音量，用于恢复 */
  lastVolume: 1,
  /** 歌词页是否展开 */
  lyricsOpen: false,
  lyrics: [] as LyricLine[],
  lyricsLoading: false,
  lyricsError: '',
});

type Listener = (action: 'load' | 'play' | 'pause' | 'seek', value?: number) => void;
let listener: Listener | null = null;

/** 防止「点了 A，A 的请求还没回来又点 B，结果 A 的响应把 B 顶掉」 */
let playToken = 0;
let lyricToken = 0;
const lyricCache = new Map<string, LyricLine[]>();

/** App.svelte 注册音频元素的桥接 */
export function bindAudio(fn: Listener) {
  listener = fn;
}

function setStatus(s: PlayStatus) {
  player.status = s;
  player.playing = s === 'playing';
  player.loading = s === 'loading';
}

function sameTrack(a: Track | null, b: Track | null): boolean {
  return !!a && !!b && a.provider === b.provider && a.id === b.id;
}

/** 由真实 <audio> 事件回写状态：不要在别处硬写 playing */
export function syncPlayback(state: 'playing' | 'paused') {
  if (state === 'playing') {
    if (player.status !== 'playing') setStatus('playing');
  } else if (player.status === 'playing' || player.status === 'loading') {
    setStatus('paused');
  }
}

/** 当前曲目是否处于「加载中」 */
export function isLoadingTrack(track: Track): boolean {
  return (
    !!player.current &&
    sameTrack(player.current, track) &&
    (player.status === 'loading' || player.loading)
  );
}

export function isCurrent(track: Track): boolean {
  return sameTrack(player.current, track);
}

/** 主按钮的语义标签 */
export function playButtonLabel(): string {
  if (!player.current) return '播放';
  switch (player.status) {
    case 'loading':
      return '加载中';
    case 'playing':
      return '暂停';
    case 'paused':
      return '继续播放';
    case 'ended':
      return '重新播放';
    case 'error':
      return '重试';
    default:
      return '播放';
  }
}

export function setMode(m: PlayMode) {
  player.mode = m;
  writeLS('play_mode', m);
}

/** 切换音质：持久化 + 若正在播放则按新音质重新取流（保持进度） */
export async function setQuality(q: Quality) {
  player.quality = q;
  writeLS('quality', q);
  const t = player.current;
  if (!t) return;
  const at = player.progress;
  const wasPlaying = player.status === 'playing';
  try {
    setStatus('loading');
    const src = await api.playerOpen(t.provider, t.id, q);
    player.src = src.url;
    listener?.('load');
    // 恢复播放位置：等 metadata 就绪后再 seek
    setTimeout(() => {
      listener?.('seek', at);
      if (wasPlaying) {
        listener?.('play');
        setStatus('playing');
      } else {
        setStatus('paused');
      }
    }, 260);
    player.error = '';
  } catch (e) {
    player.error = String(e);
    setStatus('error');
  }
}

function markUnavailable(track: Track) {
  const t = player.queue.find((x) => x.provider === track.provider && x.id === track.id);
  if (t) t.availability = 'unavailable';
}

/** 从队列第 i 首开始（内部函数，队列已就绪） */
async function startAt(i: number) {
  const track = player.queue[i];
  if (!track) return;
  const token = ++playToken;
  player.index = i;
  player.current = track;
  player.error = '';
  player.progress = 0;
  player.duration = track.duration_sec || 0;
  player.src = '';
  setStatus('loading');
  void loadLyrics(track);

  try {
    const src = await api.playerOpen(track.provider, track.id, player.quality);
    if (token !== playToken) return; // 已经被更新的点击顶替
    player.src = src.url;
    listener?.('load');
    // load 后再 play，避免 WebView 上偶发的播放被忽略
    setTimeout(() => {
      if (token !== playToken) return;
      listener?.('play');
      setStatus('playing');
    }, 30);
  } catch (e) {
    if (token !== playToken) return;
    const msg = String(e);
    player.error = msg.includes('unavailable')
      ? '这首曲目在各音质档位都没取到完整音频'
      : msg;
    setStatus('error');
    markUnavailable(track);
  }
}

export async function playTrack(track: Track, queue?: Track[]) {
  if (queue && queue.length) {
    player.queue = [...queue];
    const i = player.queue.findIndex(
      (t) => t.id === track.id && t.provider === track.provider
    );
    player.index = i >= 0 ? i : 0;
  } else {
    const i = player.queue.findIndex(
      (t) => t.id === track.id && t.provider === track.provider
    );
    if (i >= 0) {
      player.index = i;
    } else if (player.index >= 0 && player.queue.length) {
      // 不在队列里就插到当前曲目后面，保持「从列表点播」的直觉
      const at = Math.min(player.index + 1, player.queue.length);
      player.queue.splice(at, 0, track);
      player.index = at;
    } else {
      player.queue = [track];
      player.index = 0;
    }
  }
  await startAt(player.index);
}

/** 播放/暂停/继续 —— 单一入口，按当前状态决定动作 */
export function toggle() {
  if (!player.current) return;
  if (player.status === 'loading') return;
  if (player.status === 'playing') {
    listener?.('pause');
    setStatus('paused');
    return;
  }
  if (!player.src) {
    // 出错后重试、或上次没取到直链：重新取流
    void startAt(player.index >= 0 ? player.index : 0);
    return;
  }
  if (player.status === 'ended') {
    listener?.('seek', 0);
    player.progress = 0;
  }
  listener?.('play');
  setStatus('playing');
}

export function seek(sec: number) {
  listener?.('seek', sec);
  player.progress = sec;
}

export function setVolume(v: number) {
  player.volume = Math.max(0, Math.min(1, v));
  writeLS('volume', String(player.volume));
}

/** 音量面板开关（与播放列表互斥，避免两个浮层叠在一起） */
export function toggleVolume() {
  player.volumeOpen = !player.volumeOpen;
  if (player.volumeOpen) player.queueOpen = false;
}

export function closeVolume() {
  player.volumeOpen = false;
}

/** 点喇叭：有声音就静音，静音就在原音量恢复 */
export function toggleMute() {
  if (player.volume > 0) {
    player.lastVolume = player.volume;
    setVolume(0);
  } else {
    setVolume(player.lastVolume > 0 ? player.lastVolume : 0.8);
  }
}

/** 顺序/列表循环下按顺序，随机下随机挑一首 */
function nextIndex(): number | null {
  const n = player.queue.length;
  if (!n) return null;
  if (player.mode === 'shuffle') {
    if (n === 1) return 0;
    let candidate = player.index;
    // 避免随机到当前这首（列表大于 1 时）
    for (let guard = 0; guard < 20 && candidate === player.index; guard++) {
      candidate = Math.floor(Math.random() * n);
    }
    return candidate;
  }
  const i = player.index + 1;
  if (i < n) return i;
  return player.mode === 'repeat_all' ? 0 : null;
}

/** 手动下一首：总是有下一步（到头回到第一首） */
export function next() {
  const n = player.queue.length;
  if (!n) return;
  if (player.mode === 'shuffle') {
    void startAt(nextIndex() ?? 0);
    return;
  }
  void startAt((player.index + 1) % n);
}

export function prev() {
  const n = player.queue.length;
  if (!n) return;
  if (player.mode === 'shuffle') {
    void startAt(nextIndex() ?? 0);
    return;
  }
  void startAt((player.index - 1 + n) % n);
}

/** 音频自然播放结束：单曲循环自己重开，其余按模式决定下一步 */
export function onEnded() {
  if (player.mode === 'repeat_one') {
    listener?.('seek', 0);
    player.progress = 0;
    listener?.('play');
    setStatus('playing');
    return;
  }
  const i = nextIndex();
  if (i === null) {
    player.progress = player.duration;
    setStatus('ended');
    return;
  }
  void startAt(i);
}

/** 跳到播放列表第 i 首 */
export function jumpTo(i: number) {
  if (i < 0 || i >= player.queue.length) return;
  void startAt(i);
}

export function removeAt(i: number) {
  if (i < 0 || i >= player.queue.length) return;
  const removingCurrent = i === player.index;
  player.queue.splice(i, 1);

  if (player.queue.length === 0) {
    clearQueue();
    return;
  }
  if (i < player.index) player.index--;
  if (removingCurrent) {
    const target = Math.min(i, player.queue.length - 1);
    void startAt(target);
  }
}

export function clearQueue() {
  playToken++;
  listener?.('pause');
  player.queue = [];
  player.index = -1;
  player.current = null;
  player.src = '';
  player.progress = 0;
  player.duration = 0;
  player.error = '';
  player.lyrics = [];
  setStatus('idle');
}

/** 曲目是否已在播放列表里 */
export function inQueue(track: Track): boolean {
  return player.queue.some((t) => t.provider === track.provider && t.id === track.id);
}

/** 追加到播放列表末尾；没有正在播放的曲目时直接开播 */
export function addToQueue(track: Track): boolean {
  if (inQueue(track)) return false;
  if (!player.current || player.queue.length === 0) {
    void playTrack(track);
    return true;
  }
  player.queue.push(track);
  return true;
}

/** 插到当前曲目后面；如果它已经在别的位置，先挪过来 */
export function playNext(track: Track) {
  if (!player.current || player.queue.length === 0) {
    void playTrack(track);
    return;
  }
  if (sameTrack(player.current, track)) return;
  const exist = player.queue.findIndex(
    (t) => t.provider === track.provider && t.id === track.id
  );
  if (exist >= 0) {
    player.queue.splice(exist, 1);
    if (exist < player.index) player.index--;
  }
  const at = Math.min(player.index + 1, player.queue.length);
  player.queue.splice(at, 0, track);
}

export function openQueue() {
  player.queueOpen = true;
  player.volumeOpen = false;
}

export function toggleQueue() {
  player.queueOpen = !player.queueOpen;
  if (player.queueOpen) player.volumeOpen = false;
}

export function closeQueue() {
  player.queueOpen = false;
}

export function restoreVolume() {
  const v = Number(readLS('volume'));
  if (!Number.isNaN(v) && v >= 0 && v <= 1 && readLS('volume') !== '') player.volume = v;
}

// ---------- 歌词 ----------

/** 拉取歌词（带内存缓存），播放时自动预取 */
export async function loadLyrics(track: Track) {
  const key = `${track.provider}:${track.id}`;
  const cached = lyricCache.get(key);
  if (cached) {
    player.lyrics = cached;
    player.lyricsError = '';
    player.lyricsLoading = false;
    return;
  }
  const token = ++lyricToken;
  player.lyrics = [];
  player.lyricsError = '';
  player.lyricsLoading = true;
  try {
    const d = await api.detail(track.provider, track);
    if (token !== lyricToken) return;
    lyricCache.set(key, d.lyrics);
    player.lyrics = d.lyrics;
  } catch (e) {
    if (token !== lyricToken) return;
    player.lyrics = [];
    player.lyricsError = String(e);
  } finally {
    if (token === lyricToken) player.lyricsLoading = false;
  }
}

export function openLyrics() {
  if (!player.current) return;
  player.queueOpen = false;
  player.volumeOpen = false;
  player.lyricsOpen = true;
  void loadLyrics(player.current);
}

export function closeLyrics() {
  player.lyricsOpen = false;
}
