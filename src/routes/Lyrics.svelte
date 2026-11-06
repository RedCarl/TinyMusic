<script lang="ts">
  import { untrack } from 'svelte';
  import { fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { coverUrl, fmtDuration, type Quality } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import {
    player,
    closeLyrics,
    seek,
    toggle,
    next,
    prev,
    loadLyrics,
  } from '../lib/player.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { copyText } from '../lib/toast.svelte';

  let listEl = $state<HTMLElement | null>(null);
  /** 上下留白 = 半个可视高度，让任意一行都能停到正中间 */
  let centerPad = $state(120);
  /** 切歌/首次打开时直接定位，不要从顶部一路滚下来 */
  let snap = true;
  let lastScrolled = -2;

  let scrollAnim = 0;
  let followTimer: ReturnType<typeof setTimeout> | undefined;
  /** 用户自己滚歌词时暂时不要抢滚动 */
  let followPaused = false;

  const lines = $derived(player.lyrics);

  /** 当前时间对应的歌词行 */
  const active = $derived.by(() => {
    const ms = player.progress * 1000;
    let idx = -1;
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].time_ms <= ms + 60) idx = i;
      else break;
    }
    return idx;
  });

  const QUALITY_LABEL: Record<Quality, string> = {
    standard: '标准',
    high: '高品',
    super: '超高',
    lossless: '无损',
  };
  const qualityLabel = $derived(QUALITY_LABEL[player.quality] ?? '');

  function cancelAnim() {
    if (scrollAnim) cancelAnimationFrame(scrollAnim);
    scrollAnim = 0;
  }

  /** 某一行滚到视口正中间时，滚动容器应有的 scrollTop */
  function targetTop(i: number): number {
    if (!listEl) return 0;
    const el = listEl.querySelector<HTMLElement>(`[data-line="${i}"]`);
    if (!el) return listEl.scrollTop;
    const listRect = listEl.getBoundingClientRect();
    const elRect = el.getBoundingClientRect();
    const delta = elRect.top - listRect.top + elRect.height / 2 - listEl.clientHeight / 2;
    return Math.max(0, listEl.scrollTop + delta);
  }

  /** 缓动滚动，接近 iOS / Apple Music 换行的手感 */
  function scrollToLine(i: number, instant = false) {
    if (!listEl) return;
    const el = listEl;
    const top = targetTop(i);
    cancelAnim();
    if (instant || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      el.scrollTop = top;
      return;
    }
    const from = el.scrollTop;
    const dist = top - from;
    if (Math.abs(dist) < 1) return;
    const dur = Math.min(560, 240 + Math.abs(dist) * 0.35);
    const t0 = performance.now();
    const ease = (t: number) => 1 - Math.pow(1 - t, 3);
    const step = (now: number) => {
      const t = Math.min(1, (now - t0) / dur);
      el.scrollTop = from + dist * ease(t);
      scrollAnim = t < 1 ? requestAnimationFrame(step) : 0;
    };
    scrollAnim = requestAnimationFrame(step);
  }

  // 换歌：歌词列表和当前行都变了，重新来过
  $effect(() => {
    player.current; // 依赖当前曲目
    snap = true;
    lastScrolled = -2;
    followPaused = false;
    clearTimeout(followTimer);
    cancelAnim();
  });

  // 当前行变化时才滚动，避免每次 timeupdate 都触发
  $effect(() => {
    const i = active;
    if (i < 0 || i === lastScrolled) return;
    lastScrolled = i;
    if (!listEl) return;
    if (snap) {
      snap = false;
      scrollToLine(i, true);
      return;
    }
    if (followPaused) return;
    scrollToLine(i);
  });

  // 容器高度变了（窗口缩放）：重算居中留白，并把当前行重新摆正
  $effect(() => {
    const el = listEl;
    if (!el) return;
    const remeasure = () => {
      const pad = Math.round(el.clientHeight / 2);
      if (pad > 0) centerPad = pad;
      const i = untrack(() => active);
      if (i >= 0) scrollToLine(i, true);
    };
    remeasure();
    const ro = new ResizeObserver(remeasure);
    ro.observe(el);
    return () => {
      ro.disconnect();
      cancelAnim();
      clearTimeout(followTimer);
    };
  });

  /** 用户滚歌词：暂停跟随，停手几秒后回到当前行 */
  function onUserScroll() {
    followPaused = true;
    clearTimeout(followTimer);
    followTimer = setTimeout(() => {
      followPaused = false;
      const i = active;
      if (i >= 0) scrollToLine(i);
    }, 3200);
  }

  function retryLyrics() {
    if (player.current) void loadLyrics(player.current);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeLyrics();
    }
    if (e.code === 'Space' && e.target === document.body) {
      e.preventDefault();
      toggle();
    }
  }

  const SKELETON = [66, 84, 52, 78, 61, 88, 47];
  const REDUCE_MOTION = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
</script>

<svelte:window onkeydown={onKey} />

<div
  class="lyrics-page"
  role="dialog"
  aria-modal="true"
  aria-label="歌词"
  transition:fly={{ y: REDUCE_MOTION ? 0 : '100%', duration: REDUCE_MOTION ? 0 : 320, opacity: 1 }}
>
  <div class="lyrics-bg" aria-hidden="true">
    {#if player.current?.cover}
      <img class="lyrics-bg-img" src={coverUrl(player.current.cover, 900)} alt="" />
    {/if}
  </div>

  <!-- macOS Overlay 标题栏下，留出红绿灯并让顶部可以拖动窗口 -->
  <div class="lyrics-drag" data-tauri-drag-region aria-hidden="true"></div>

  <header class="lyrics-top">
    <button class="ly-back" onclick={closeLyrics} aria-label="退出歌词页">
      <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
      <span>返回</span>
    </button>

    <span class="lyrics-top-label">
      {#if player.status === 'playing'}
        <span class="bars ly-bars" aria-hidden="true"><i></i><i></i><i></i></span>
      {/if}
      正在播放
    </span>

    <div class="lyrics-top-actions">
      {#if qualityLabel}
        <span class="ly-quality" title="当前播放音质">{qualityLabel}</span>
      {/if}
    </div>
  </header>

  <div class="lyrics-body">
    <div class="lp-side">
      <div class="lp-art">
        {#if player.current?.cover}
          <img class="lp-cover" src={coverUrl(player.current.cover, 800)} alt="" />
        {:else}
          <div class="lp-cover ph" aria-hidden="true">
            <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
          </div>
        {/if}
      </div>

      <div class="lp-meta">
        <h1 class="lp-title" title={player.current?.title ?? ''}>{player.current?.title ?? ''}</h1>
        <div class="lp-artist" title={player.current?.artist ?? ''}>
          {player.current?.artist ?? ''}
        </div>
        {#if player.current?.album}
          <div class="lp-album" title={player.current.album}>{player.current.album}</div>
        {/if}
      </div>

      <div class="lp-ctrls">
        <button class="c" onclick={prev} title="上一首" aria-label="上一首">
          <HugeiconsIcon icon={icons.previous} size={ICON_SIZE.control} />
        </button>
        <button
          class="c main"
          onclick={toggle}
          disabled={player.status === 'loading'}
          title={player.status === 'playing' ? '暂停' : '继续播放'}
          aria-label={player.status === 'playing' ? '暂停' : '继续播放'}
        >
          {#if player.status === 'loading'}
            <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.control} /></span>
          {:else if player.status === 'playing'}
            <HugeiconsIcon icon={icons.pause} size={ICON_SIZE.control} />
          {:else}
            <HugeiconsIcon icon={icons.play} size={ICON_SIZE.control} />
          {/if}
        </button>
        <button class="c" onclick={next} title="下一首" aria-label="下一首">
          <HugeiconsIcon icon={icons.next} size={ICON_SIZE.control} />
        </button>
      </div>

      <div class="lp-progress">
        <input
          type="range"
          min="0"
          max={Math.max(1, Math.floor(player.duration))}
          value={Math.floor(player.progress)}
          style="--p:{player.duration > 0 ? (player.progress / player.duration) * 100 : 0}%"
          oninput={(e) => seek(Number((e.target as HTMLInputElement).value))}
          aria-label="播放进度"
        />
        <div class="lp-times">
          <span>{fmtDuration(player.progress)}</span>
          <span>-{fmtDuration(Math.max(0, player.duration - player.progress))}</span>
        </div>
      </div>
    </div>

    <section class="lyrics-main" aria-label="歌词">
      {#if player.lyricsLoading && lines.length === 0}
        <div class="ly-skel" role="status" aria-label="歌词加载中">
          {#each SKELETON as w}
            <span class="sk ly-skel-line" style="width:{w}%"></span>
          {/each}
        </div>
      {:else if lines.length === 0}
        <div class="ly-empty">
          <span class="ly-empty-ico" aria-hidden="true">
            <HugeiconsIcon icon={icons.lyric} size={ICON_SIZE.empty} />
          </span>
          <p>{player.lyricsError ? '歌词加载失败' : '这首歌暂时没有歌词'}</p>
          {#if player.lyricsError}<span class="ly-empty-sub">{player.lyricsError}</span>{/if}
          {#if player.current}
            <button class="ly-retry" onclick={retryLyrics}>
              <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
              重新获取
            </button>
          {/if}
        </div>
      {:else}
        <div
          class="lyrics-scroll"
          bind:this={listEl}
          style="--pad:{centerPad}px"
          role="group"
          aria-label="歌词内容"
          onwheel={onUserScroll}
          ontouchstart={onUserScroll}
        >
          {#each lines as l, i (l.time_ms + '-' + i)}
            {@const d = i - active}
            <button
              class="ly"
              class:on={d === 0}
              class:d1={Math.abs(d) === 1}
              class:d2={Math.abs(d) === 2}
              class:d3={Math.abs(d) >= 3}
              data-line={i}
              aria-current={d === 0 ? 'true' : undefined}
              onclick={() => seek(l.time_ms / 1000)}
              oncontextmenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                openMenuAt(e.clientX, e.clientY, [
                  {
                    label: '从这一句开始播放',
                    icon: icons.play,
                    run: () => seek(l.time_ms / 1000),
                  },
                  {
                    label: '复制这句歌词',
                    icon: icons.link,
                    run: () => copyText(l.text, '已复制歌词'),
                  },
                ]);
              }}
              title="跳转到这一句"
            >
              {l.text || '…'}
            </button>
          {/each}
        </div>
      {/if}
    </section>
  </div>
</div>
