<script lang="ts">
  /**
   * 根据 uid / 昵称生成一个稳定的几何头像。
   *
   * 同一串 seed 永远得到同一张脸：底色从 8 组灰阶偏色的渐变里挑一组，
   * 前景是 5×5 左右对称的圆角方块（identicon）。不依赖网络图片，
   * 也不把 uid 的第一个字符直接印在头像上。
   */

  interface Props {
    seed: string;
    size?: number;
    /** 有值时作为图片的替代文本，否则视为纯装饰 */
    label?: string;
  }

  let { seed, size = 28, label = '' }: Props = $props();

  const PALETTES: [string, string][] = [
    ['#7183e6', '#3d469c'],
    ['#51a4a6', '#26676c'],
    ['#d29a5a', '#8a5a28'],
    ['#d27a9c', '#853f60'],
    ['#68b57c', '#2e7049'],
    ['#8b7bd8', '#4f3c9e'],
    ['#5d9fd8', '#2c5d8f'],
    ['#d47c62', '#8f3d2b']
  ];

  function hashSeed(s: string): number {
    let h = 2166136261 >>> 0;
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 16777619) >>> 0;
    }
    return h >>> 0;
  }

  /** 小而稳定的 PRNG，保证每次渲染的图案一致 */
  function mulberry32(a: number): () => number {
    return () => {
      a |= 0;
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  const art = $derived.by(() => {
    const key = seed || 'tinymusic';
    const hash = hashSeed(key);
    const [from, to] = PALETTES[hash % PALETTES.length];
    const rand = mulberry32(hash);
    const gradId = `gav-${hash.toString(36)}`;

    let cells: boolean[] = [];
    let filled = 0;
    for (let attempt = 0; attempt < 16; attempt++) {
      const next = new Array<boolean>(25).fill(false);
      let count = 0;
      for (let row = 0; row < 5; row++) {
        // 只生成左半边三列，右边镜像，图案天然左右对称
        const left = [rand() < 0.52, rand() < 0.52, rand() < 0.52];
        for (let col = 0; col < 5; col++) {
          const source = col < 3 ? col : 4 - col;
          const on = left[source];
          next[row * 5 + col] = on;
          if (on) count++;
        }
      }
      cells = next;
      filled = count;
      if (count >= 8 && count <= 16) break;
    }

    // 图案太满时留出一点呼吸；太稀时补两块，避免看起来像脏点
    if (filled < 6) {
      for (let row = 0; row < 5 && filled < 8; row++) {
        if (!cells[row * 5 + 2]) {
          cells[row * 5 + 1] = true;
          cells[row * 5 + 2] = true;
          cells[row * 5 + 3] = true;
          filled += 3;
        }
      }
    }

    const rects: { x: number; y: number; key: string }[] = [];
    const origin = 5.2;
    const pitch = 4.4;
    for (let row = 0; row < 5; row++) {
      for (let col = 0; col < 5; col++) {
        if (!cells[row * 5 + col]) continue;
        rects.push({
          x: origin + col * pitch,
          y: origin + row * pitch,
          key: `${row}-${col}`
        });
      }
    }

    return { from, to, gradId, rects };
  });
</script>

<svg
  class="generated-avatar"
  width={size}
  height={size}
  viewBox="0 0 32 32"
  role={label ? 'img' : undefined}
  aria-label={label || undefined}
  aria-hidden={label ? undefined : 'true'}
  focusable="false"
>
  <defs>
    <linearGradient id={art.gradId} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color={art.from} />
      <stop offset="100%" stop-color={art.to} />
    </linearGradient>
  </defs>
  <rect width="32" height="32" rx="16" fill="url(#{art.gradId})" />
  {#each art.rects as r (r.key)}
    <rect x={r.x} y={r.y} width="4" height="4" rx="1.15" fill="#fff" opacity="0.9" />
  {/each}
</svg>

<style>
  .generated-avatar {
    display: block;
    flex: none;
  }
</style>
