<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { coverUrl, fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { player, closeLyrics, seek, toggle, next, prev } from '../lib/player.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { copyText } from '../lib/toast.svelte';

  let listEl = $state<HTMLElement | null>(null);
  let lastScrolled = -2;

  const lines = $derived(player.lyrics);

  /** 当前时间对应的歌词行 */
  function activeIndex(): number {
    const ms = player.progress * 1000;
    let idx = -1;
    for (let i = 0; i < lines.length; i++) {
      if (lines[i].time_ms <= ms + 60) idx = i;
      else break;
    }
    return idx;
  }

  const active = $derived(activeIndex());

  // 歌词自动滚动：只在「当前行变化」时滚动，避免每次 timeupdate 都触发
  $effect(() => {
    const i = active;
    if (i < 0 || i === lastScrolled) return;
    lastScrolled = i;
    if (!listEl) return;
    const el = listEl.querySelector<HTMLElement>(`[data-line="${i}"]`);
    if (!el) return;
    const top = el.offsetTop - listEl.clientHeight / 2 + el.clientHeight / 2;
    listEl.scrollTo({ top: Math.max(0, top), behavior: 'smooth' });
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') closeLyrics();
    if (e.code === 'Space' && e.target === document.body) {
      e.preventDefault();
      toggle();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="lyrics-page" role="dialog" aria-modal="true" aria-label="歌词">
  {#if player.current?.cover}
    <div
      class="lyrics-bg"
      style="background-image:url({coverUrl(player.current.cover, 800)})"
      aria-hidden="true"
    ></div>
  {/if}

  <div class="lyrics-top">
    <button class="back" onclick={closeLyrics} aria-label="返回">
      <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
      返回
    </button>
  </div>

  <div class="lyrics-body">
    <div class="lp-side">
      {#if player.current?.cover}
        <img class="lp-cover" src={coverUrl(player.current.cover, 800)} alt="" />
      {:else}
        <div class="lp-cover ph" aria-hidden="true">
          <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
        </div>
      {/if}
      <h1 class="lp-title">{player.current?.title ?? ''}</h1>
      <div class="lp-artist">{player.current?.artist ?? ''}</div>
      {#if player.current?.album}
        <div class="lp-album">{player.current.album}</div>
      {/if}
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
    </div>

    <div class="lyrics-main">
      {#if player.lyricsLoading}
        <div class="lp-tip">歌词加载中…</div>
      {:else if lines.length === 0}
        <div class="lp-tip">
          {player.lyricsError ? `歌词获取失败：${player.lyricsError}` : '这首歌暂时没有歌词'}
        </div>
      {:else}
        <div class="lyrics-scroll" bind:this={listEl}>
          {#each lines as l, i (l.time_ms + '-' + i)}
            <button
              class="ly"
              class:on={i === active}
              class:past={i < active}
              data-line={i}
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
          <div class="ly-pad"></div>
        </div>
      {/if}
      <div class="lp-progress">
        <span>{fmtDuration(player.progress)}</span>
        <input
          type="range"
          min="0"
          max={Math.max(1, Math.floor(player.duration))}
          value={Math.floor(player.progress)}
          style="--p:{player.duration > 0 ? (player.progress / player.duration) * 100 : 0}%"
          oninput={(e) => seek(Number((e.target as HTMLInputElement).value))}
          aria-label="播放进度"
        />
        <span>{fmtDuration(player.duration)}</span>
      </div>
    </div>
  </div>
</div>
