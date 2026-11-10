import { api, type LyricLine, type Quality, type Track } from './api';

/** 播放状态：播放按钮据此显示「播放 / 暂停 / 继续播放 / 加载中」 */
export type PlayStatus = 'idle' | 'loading' | 'playing' | 'paused' | 'ended' | 'error';

/** 循环方式：不循环 / 列表循环 / 单曲循环 */
export type RepeatMode = 'off' | 'all' | 'one';

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

function readShuffle(): boolean {
  const v = readLS('shuffle');
  if (v === '1' || v === '0') return v === '1';
  return readLS('play_mode') === 'shuffle';
}

function readRepeat(): RepeatMode {
  const v = readLS('repeat');
  if (v === 'off' || v === 'all' || v === 'one') return v;
  const old = readLS('play_mode');
  if (old === 'repeat_all') return 'all';
  if (old === 'repeat_one') return 'one';
  return 'off';
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
  /** 随机播放（和循环相互独立，和 Apple Music 一致） */
  shuffle: readShuffle(),
  /** 循环方式 */
  repeat: readRepeat() as RepeatMode,
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

/** 上次播放位置：本地缓存，只用于「下次打开时恢复」，不会自动播放 */
interface PlaybackRecordV1 {
  v: 1;
  current: Track;
  queue: Track[];
  index: number;
  progress: number;
  duration: number;
  at: number;
}

const PLAYBACK_KEY = 'playback';
const PLAYBACK_QUEUE_MAX = 500;
let lastPersistAt = 0;
/** 流加载完成后需要跳到的位置；由 App 在 loadedmetadata 里取走 */
let pendingSeek: number | null = null;

function isTrack(x: unknown): x is Track {
  const t = x as Track;
  return !!t && typeof t.provider === 'string' && typeof t.id === 'string' && typeof t.title === 'string';
}

function restorePlayback() {
  try {
    const raw = readLS(PLAYBACK_KEY);
    if (!raw) return;
    const rec = JSON.parse(raw) as Partial<PlaybackRecordV1>;
    if (!rec || !isTrack(rec.current) || !Array.isArray(rec.queue)) return;

    const current = rec.current;
    const queue = rec.queue.filter(isTrack);
    if (!queue.some((t) => sameTrack(t, current))) queue.unshift(current);
    let index = typeof rec.index === 'number' ? rec.index : -1;
    if (index < 0 || index >= queue.length || !sameTrack(queue[index], current)) {
      index = queue.findIndex((t) => sameTrack(t, current));
    }
    if (index < 0) return;

    const duration = Number(rec.duration) || queue[index].duration_sec || 0;
    let progress = Math.max(0, Number(rec.progress) || 0);
    // 上次关在结尾附近时，从 0 开始，避免一按播放就结束
    if (duration > 0 && progress > duration - 2) progress = 0;
    if (duration > 0) progress = Math.min(progress, duration);

    player.queue = queue;
    player.index = index;
    player.current = queue[index];
    player.duration = duration;
    player.progress = progress;
    player.src = '';
    setStatus('paused');
  } catch {
    /* 记录损坏就当没有 */
  }
}

/**
 * 保存播放记录。默认节流 3 秒；pause / 切歌 / 页面隐藏时传 force。
 * 只存当前曲目、队列、进度和时长，音量/音质/随机/循环各自已有持久化。
 */
export function persistPlayback(force = false) {
  if (!player.current) return;
  const now = Date.now();
  if (!force && now - lastPersistAt < 2000) return;
  lastPersistAt = now;

  let queue = player.queue.length ? player.queue.slice() : [player.current];
  let index = player.index;
  if (index < 0 || index >= queue.length) {
    index = queue.findIndex((t) => sameTrack(t, player.current));
  }
  if (index < 0) {
    queue = [player.current];
    index = 0;
  }
  if (queue.length > PLAYBACK_QUEUE_MAX) {
    const start = Math.max(
      0,
      Math.min(index - Math.floor(PLAYBACK_QUEUE_MAX / 2), queue.length - PLAYBACK_QUEUE_MAX)
    );
    queue = queue.slice(start, start + PLAYBACK_QUEUE_MAX);
    index -= start;
  }

  const rec: PlaybackRecordV1 = {
    v: 1,
    current: player.current,
    queue,
    index,
    progress: player.progress,
    duration: player.duration || player.current.duration_sec || 0,
    at: now,
  };
  writeLS(PLAYBACK_KEY, JSON.stringify(rec));
}

export function clearPersistedPlayback() {
  try {
    localStorage.removeItem(PLAYBACK_KEY);
    lastPersistAt = 0;
  } catch {
    /* ignore */
  }
}

/** App 在 audio.loadedmetadata 里取走待恢复的进度，执行 seek + play */
export function consumePendingSeek(): number | null {
  const s = pendingSeek;
  pendingSeek = null;
  return s;
}

export function clearPendingSeek() {
  pendingSeek = null;
}

restorePlayback();

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

export function setShuffle(on: boolean) {
  player.shuffle = on;
  writeLS('shuffle', on ? '1' : '0');
}

export function toggleShuffle() {
  setShuffle(!player.shuffle);
}

export function setRepeat(r: RepeatMode) {
  player.repeat = r;
  writeLS('repeat', r);
}

/** 循环按钮：不循环 → 列表循环 → 单曲循环 → 不循环 */
export function cycleRepeat() {
  setRepeat(player.repeat === 'off' ? 'all' : player.repeat === 'all' ? 'one' : 'off');
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

/** 从队列第 i 首开始；fromSec > 0 用于恢复上次进度（等 metadata 后再 seek + play） */
async function startAt(i: number, fromSec = 0) {
  const track = player.queue[i];
  if (!track) return;
  const token = ++playToken;
  const resume = fromSec > 0;
  player.index = i;
  player.current = track;
  player.error = '';
  player.progress = resume ? fromSec : 0;
  player.duration = track.duration_sec || 0;
  player.src = '';
  setStatus('loading');
  persistPlayback(true);
  void loadLyrics(track);

  try {
    const src = await api.playerOpen(track.provider, track.id, player.quality);
    if (token !== playToken) return; // 已经被更新的点击顶替
    player.src = src.url;
    pendingSeek = resume ? fromSec : null;
    listener?.('load');
    if (resume) {
      // App 的 loadedmetadata 会取走 pendingSeek：先 seek 到上次位置，再播放
      return;
    }
    // load 后再 play，避免 WebView 上偶发的播放被忽略
    setTimeout(() => {
      if (token !== playToken) return;
      listener?.('play');
      setStatus('playing');
    }, 30);
  } catch (e) {
    if (token !== playToken) return;
    pendingSeek = null;
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
    // 出错后重试、或恢复了上次记录还没取流：从当前进度继续
    void startAt(player.index >= 0 ? player.index : 0, player.progress);
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
  persistPlayback();
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
  if (player.shuffle) {
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
  return player.repeat === 'all' ? 0 : null;
}

/** 手动下一首：总是有下一步（到头回到第一首） */
export function next() {
  const n = player.queue.length;
  if (!n) return;
  if (player.shuffle) {
    void startAt(nextIndex() ?? 0);
    return;
  }
  void startAt((player.index + 1) % n);
}

export function prev() {
  const n = player.queue.length;
  if (!n) return;
  if (player.shuffle) {
    void startAt(nextIndex() ?? 0);
    return;
  }
  void startAt((player.index - 1 + n) % n);
}

/** 音频自然播放结束：单曲循环自己重开，其余按循环方式决定下一步 */
export function onEnded() {
  if (player.repeat === 'one') {
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
  } else {
    persistPlayback(true);
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
  player.queueOpen = false;
  player.volumeOpen = false;
  clearPersistedPlayback();
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
  persistPlayback();
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
  persistPlayback();
}

/** 刚关掉播放列表的时间点：连点两下时，第二次不应该马上又把它打开 */
let lastQueueCloseAt = 0;
const QUEUE_REOPEN_GUARD = 220;

export function openQueue() {
  player.queueOpen = true;
  player.volumeOpen = false;
}

export function toggleQueue() {
  if (!player.queueOpen) {
    // 关闭动画还在走的时候（~180ms）忽略重新打开，避免「开了又关、关了又开」
    if (Date.now() - lastQueueCloseAt < QUEUE_REOPEN_GUARD) return;
    player.queueOpen = true;
    player.volumeOpen = false;
    return;
  }
  closeQueue();
}

export function closeQueue() {
  if (player.queueOpen) lastQueueCloseAt = Date.now();
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
