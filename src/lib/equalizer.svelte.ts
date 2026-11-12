/**
 * 均衡器：10 段图形均衡 + 预设。
 *
 * 声音处理走 Web Audio：把 `<audio>` 接进一条
 *   前级 → 10 个 BiquadFilter → 音量 → 峰值限幅 → 输出
 * 的链子。只有在「开启」时才会建立这条链，关闭时不碰音频元素，
 * 避免任何平台上的兼容性风险。关闭后链子保留但全部归零（透明）。
 *
 * 跨域：音频统一走本地代理，代理带 `Access-Control-Allow-Origin: *`，
 * 所以 `<audio>` 必须带 `crossorigin="anonymous"`，否则 Web Audio 会拿到静音。
 */

export interface EqualizerPreset {
  id: string;
  name: string;
  desc: string;
  gains: number[];
  preamp: number;
}

/** 频段：32 / 64 / 125 / ... / 16k，标签只写数字，单位在面板上说明。 */
export const EQ_BANDS = [
  { hz: 32, label: '32' },
  { hz: 64, label: '64' },
  { hz: 125, label: '125' },
  { hz: 250, label: '250' },
  { hz: 500, label: '500' },
  { hz: 1000, label: '1k' },
  { hz: 2000, label: '2k' },
  { hz: 4000, label: '4k' },
  { hz: 8000, label: '8k' },
  { hz: 16000, label: '16k' }
] as const;

export const EQ_MIN_DB = -12;
export const EQ_MAX_DB = 12;

const FLAT_GAINS = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/**
 * 预设曲线。数字都按「听感」调过：
 * 低音 / 响度这类大增益的预设会给一个负的前级（留 headroom），
 * 限幅器只在真的有正增益时介入。
 */
export const EQ_PRESETS: EqualizerPreset[] = [
  {
    id: 'flat',
    name: '平直',
    desc: '不染色，所有频段 0 dB',
    preamp: 0,
    gains: FLAT_GAINS
  },
  {
    id: 'pop',
    name: '流行',
    desc: '人声和中高频更靠前，低频收一点',
    preamp: -3,
    gains: [-1, 0, 2, 3.5, 3.5, 2, 0.5, -1, -1.5, -2]
  },
  {
    id: 'rock',
    name: '摇滚',
    desc: '经典微笑曲线：鼓和贝斯更重，吉他更亮',
    preamp: -4,
    gains: [4, 3, 2, 0, -1, -1, 0, 2, 3, 4]
  },
  {
    id: 'jazz',
    name: '爵士',
    desc: '温暖的低频和铜管，中频略退',
    preamp: -3,
    gains: [3, 2, 0, 1, -1, -1, 0, 1, 2, 2.5]
  },
  {
    id: 'classical',
    name: '古典',
    desc: '保留厅堂感，弦乐和空气感更清晰',
    preamp: -3,
    gains: [3.5, 3, 2, 1, 0, 0, -0.5, 0, 2, 3]
  },
  {
    id: 'electronic',
    name: '电子',
    desc: '下潜和 hi-hat 都更突出',
    preamp: -4,
    gains: [5, 4, 1, -1, -2, 0, 1, 3, 4, 5]
  },
  {
    id: 'hiphop',
    name: '嘻哈',
    desc: '808 和底鼓更厚，顶部保留咬字',
    preamp: -5,
    gains: [6, 5, 2, 0, -1, 0, 1, 2, 3, 3]
  },
  {
    id: 'vocal',
    name: '人声',
    desc: '突出中频，压低低频和齿音',
    preamp: -3,
    gains: [-2, -2, -1, 1, 3, 4, 4, 2, 0, -1]
  },
  {
    id: 'acoustic',
    name: '原声',
    desc: '木吉他和人声都自然、靠前',
    preamp: -2,
    gains: [3, 2, 1, 1, 2, 2, 1.5, 1, 1.5, 2]
  },
  {
    id: 'bass',
    name: '低音增强',
    desc: '只抬低频，适合鼓点和小音箱',
    preamp: -7,
    gains: [8, 7, 5, 3, 1, 0, 0, 0, 0, 0]
  },
  {
    id: 'treble',
    name: '高音增强',
    desc: '更亮的高频，适合闷的耳机',
    preamp: -4,
    gains: [0, 0, 0, 0, 0, 0, 1, 3, 5, 6]
  },
  {
    id: 'loudness',
    name: '响度',
    desc: '小音量下补足两端，中频略退',
    preamp: -6,
    gains: [6, 4, 1, -2, -1, 1, 3, 4, 5, 5]
  }
];

const STORAGE_KEY = 'tinymusic.equalizer';

interface StoredEqualizer {
  enabled?: boolean;
  preset?: string;
  preamp?: number;
  gains?: number[];
}

function clampDb(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, Math.round(v * 2) / 2));
}

function readStored(): StoredEqualizer {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? (JSON.parse(raw) as StoredEqualizer) : {};
  } catch {
    return {};
  }
}

function storedGains(raw: StoredEqualizer): number[] {
  if (!Array.isArray(raw.gains) || raw.gains.length !== EQ_BANDS.length) return [...FLAT_GAINS];
  return raw.gains.map((v) => clampDb(Number(v)));
}

const stored = readStored();
const storedGainValues = storedGains(stored);

const storedPreset =
  stored.preset === 'custom'
    ? 'custom'
    : EQ_PRESETS.some((p) => p.id === stored.preset)
      ? stored.preset!
      : detectPreset(storedGainValues);

export const equalizer = $state({
  /** 面板是否展开 */
  open: false,
  /** 是否启用处理（持久化） */
  enabled: stored.enabled === true,
  /** 当前预设 id；手动改滑块后变成 custom */
  preset: storedPreset,
  /** 前级增益，dB */
  preamp: clampDb(Number(stored.preamp ?? 0)),
  /** 10 段增益，dB */
  gains: storedGainValues,
  /** 当前系统是否拒绝建立 Web Audio 链（失败后置 true，界面给出说明） */
  unsupported: false,
  /** 音频链是否已建立：建立后音量改由链尾的增益节点控制 */
  graphReady: false
});

// ---------------------------------------------------------------------------
// Web Audio 链
// ---------------------------------------------------------------------------

let audioEl: HTMLAudioElement | null = null;
let audioCtx: AudioContext | null = null;
let preampNode: GainNode | null = null;
let filterNodes: BiquadFilterNode[] = [];
let limiterNode: DynamicsCompressorNode | null = null;
let volumeNode: GainNode | null = null;
let pendingVolume = 1;

function dbToGain(db: number): number {
  return Math.pow(10, db / 20);
}

/**
 * 把 `<audio>` 交给均衡器。只记录元素，不在这里建 AudioContext：
 * 建立必须发生在用户手势里，否则上下文会一直 suspended，声音会消失。
 */
export function bindEqualizerAudio(el: HTMLAudioElement) {
  audioEl = el;
  // 必须在设置 src 之前声明；代理已经带了 CORS 头
  try {
    el.crossOrigin = 'anonymous';
  } catch {
    /* 极老的 WebView 可能不支持，忽略 */
  }
}

function makeFilter(ctx: AudioContext, index: number): BiquadFilterNode {
  const band = EQ_BANDS[index];
  const node = ctx.createBiquadFilter();
  node.type = index === 0 ? 'lowshelf' : index === EQ_BANDS.length - 1 ? 'highshelf' : 'peaking';
  node.frequency.value = band.hz;
  // 10 段基本是 1 个倍频程一格，Q ≈ 1.2 不会互相吃掉
  node.Q.value = 1.2;
  node.gain.value = 0;
  return node;
}

/** 建立音频链；返回 false 表示当前环境不支持 Web Audio。 */
function ensureEqualizerGraph(): boolean {
  if (audioCtx) return true;
  const el = audioEl;
  if (!el) return false;

  let ctx: AudioContext | null = null;
  try {
    const AudioCtor =
      window.AudioContext ??
      (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtor) return false;

    ctx = new AudioCtor();
    const source = ctx.createMediaElementSource(el);
    const preamp = ctx.createGain();
    const filters = EQ_BANDS.map((_, i) => makeFilter(ctx!, i));
    const limiter = ctx.createDynamicsCompressor();
    // 关闭时阈值 0 dB，等于不压缩；真的抬了频段才降到 -1.5 dB 兜底
    limiter.threshold.value = 0;
    limiter.knee.value = 0;
    limiter.ratio.value = 12;
    limiter.attack.value = 0.003;
    limiter.release.value = 0.25;

    // 链子建好后音量统一交给这个节点：不依赖 WebView 是否把
    // <audio> 自身的 volume 应用在 MediaElementAudioSourceNode 之前。
    const volume = ctx.createGain();
    volume.gain.value = pendingVolume;

    source.connect(preamp);
    let last: AudioNode = preamp;
    for (const f of filters) {
      last.connect(f);
      last = f;
    }
    // 音量放在限幅器之前：音量小的时候限幅器看到的就是最终输出电平，
    // 不会因为「抬了频段但用户把音量调小」而继续压缩。
    last.connect(volume);
    volume.connect(limiter);
    limiter.connect(ctx.destination);

    audioCtx = ctx;
    preampNode = preamp;
    filterNodes = filters;
    limiterNode = limiter;
    volumeNode = volume;
    // 链已经接好，元素自身保持 1，音量全部由上面的增益节点负责
    el.volume = 1;
    equalizer.graphReady = true;
    applyEqualizer();
    void ctx.resume().catch(() => {});
    return true;
  } catch {
    try {
      void ctx?.close();
    } catch {
      /* ignore */
    }
    // 建链失败就把元素音量恢复成用户当前音量
    if (audioEl) audioEl.volume = pendingVolume;
    audioCtx = null;
    preampNode = null;
    filterNodes = [];
    limiterNode = null;
    volumeNode = null;
    equalizer.graphReady = false;
    return false;
  }
}

function maxPositiveBoost(): number {
  let max = equalizer.preamp;
  for (const g of equalizer.gains) max = Math.max(max, g);
  return max;
}

/**
 * 音量保持同步：没有建链时记下来，建链后由链尾增益执行。
 * App 的 volume effect 会一直调用它，所以建链瞬间也是正确的音量。
 */
export function setEqualizerVolume(v: number) {
  pendingVolume = Math.max(0, Math.min(1, v));
  if (audioCtx && volumeNode) {
    volumeNode.gain.setTargetAtTime(pendingVolume, audioCtx.currentTime, 0.015);
  }
}

/** 把当前状态推给音频节点。没有建链时是安全的空操作。 */
export function applyEqualizer() {
  const ctx = audioCtx;
  if (!ctx || !preampNode || !limiterNode) return;

  const now = ctx.currentTime;
  const on = equalizer.enabled && !equalizer.unsupported;

  preampNode.gain.setTargetAtTime(dbToGain(on ? equalizer.preamp : 0), now, 0.02);
  filterNodes.forEach((node, i) => {
    const db = on ? clampDb(equalizer.gains[i] ?? 0) : 0;
    node.gain.setTargetAtTime(db, now, 0.02);
  });
  // 只有真的存在正增益时才限幅；平直/纯衰减时不介入，保持完全透明
  limiterNode.threshold.setTargetAtTime(on && maxPositiveBoost() > 0.01 ? -1.5 : 0, now, 0.02);
}

/**
 * 在用户手势里建链并把上下文拉回 running。
 *
 * `allowCreate = false` 用在自动播放 / 托盘播放这类没有页面手势的路径：
 * 此时如果链还没建好就先不动音频，等下一次真实点击再接入，避免
 * AudioContext 因为自动播放策略一直 suspended 把声音吞掉。
 */
export function prepareEqualizerForPlay(allowCreate = true): 'ok' | 'off' | 'failed' | 'wait' {
  if (!equalizer.enabled || equalizer.unsupported) return 'off';
  if (!audioCtx && !allowCreate) return 'wait';
  if (!ensureEqualizerGraph()) {
    equalizer.unsupported = true;
    equalizer.enabled = false;
    persistNow();
    return 'failed';
  }
  resumeEqualizerAudio();
  return 'ok';
}

export function resumeEqualizerAudio() {
  if (audioCtx && audioCtx.state === 'suspended') {
    void audioCtx.resume().catch(() => {});
  }
}

// ---------------------------------------------------------------------------
// 持久化
// ---------------------------------------------------------------------------

let persistTimer: ReturnType<typeof setTimeout> | null = null;

function persistNow() {
  if (persistTimer) {
    clearTimeout(persistTimer);
    persistTimer = null;
  }
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({
        enabled: equalizer.enabled,
        preset: equalizer.preset,
        preamp: equalizer.preamp,
        gains: equalizer.gains
      })
    );
  } catch {
    /* 隐私模式忽略 */
  }
}

function schedulePersist() {
  if (persistTimer) clearTimeout(persistTimer);
  persistTimer = setTimeout(persistNow, 180);
}

// 拖动滑块时持久化是防抖的；关窗口前把最后一次改动落盘
if (typeof window !== 'undefined') {
  window.addEventListener('pagehide', persistNow);
  window.addEventListener('beforeunload', persistNow);
}

// ---------------------------------------------------------------------------
// 操作
// ---------------------------------------------------------------------------

export function openEqualizer() {
  equalizer.open = true;
}

export function closeEqualizer() {
  equalizer.open = false;
}

export function toggleEqualizerPanel() {
  equalizer.open = !equalizer.open;
}

/** 预设与当前增益完全一致时返回它，否则返回 custom。 */
function detectPreset(gains: number[]): string {
  const hit = EQ_PRESETS.find(
    (p) => p.gains.length === gains.length && p.gains.every((g, i) => Math.abs(g - gains[i]) < 0.01)
  );
  return hit?.id ?? 'custom';
}

export function setPreset(id: string) {
  const preset = EQ_PRESETS.find((p) => p.id === id);
  if (!preset) return;
  equalizer.preset = preset.id;
  equalizer.gains = [...preset.gains];
  equalizer.preamp = preset.preamp;
  applyEqualizer();
  schedulePersist();
}

export function setBandGain(index: number, db: number) {
  if (index < 0 || index >= EQ_BANDS.length) return;
  equalizer.gains[index] = clampDb(db);
  equalizer.preset = detectPreset(equalizer.gains);
  applyEqualizer();
  schedulePersist();
}

export function setPreamp(db: number) {
  equalizer.preamp = clampDb(db);
  applyEqualizer();
  schedulePersist();
}

export function resetEqualizer() {
  setPreset('flat');
}

export function setEqualizerEnabled(next: boolean): boolean {
  if (next) {
    if (!ensureEqualizerGraph()) {
      // 没有音频元素时先记下状态，等真正播放（用户手势）时再建链
      if (audioEl) {
        equalizer.unsupported = true;
        equalizer.enabled = false;
        persistNow();
        return false;
      }
      equalizer.enabled = true;
      persistNow();
      return true;
    }
    equalizer.enabled = true;
    equalizer.unsupported = false;
    applyEqualizer();
    resumeEqualizerAudio();
  } else {
    equalizer.enabled = false;
    applyEqualizer();
  }
  persistNow();
  return true;
}

export function currentPresetName(): string {
  return EQ_PRESETS.find((p) => p.id === equalizer.preset)?.name ?? '自定义';
}

export function formatDb(db: number): string {
  if (Math.abs(db) < 0.001) return '0';
  const rounded = Math.round(db * 2) / 2;
  return `${rounded > 0 ? '+' : ''}${Number.isInteger(rounded) ? rounded : rounded.toFixed(1)}`;
}
