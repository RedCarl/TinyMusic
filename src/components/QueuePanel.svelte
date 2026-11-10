<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { openTrackMenu } from '../lib/trackMenu.svelte';
  import {
    player,
    jumpTo,
    removeAt,
    clearQueue,
    closeQueue,
  } from '../lib/player.svelte';

  let listEl = $state<HTMLElement | null>(null);
  let wasOpen = false;

  /** 把正在播放的那一行滚到列表中间（刚打开时直接定位，之后换歌才用动画） */
  function centerCurrent(instant: boolean) {
    const list = listEl;
    if (!list) return;
    const row = list.querySelector<HTMLElement>('.qp-row.on');
    if (!row) return;
    const listRect = list.getBoundingClientRect();
    const rowRect = row.getBoundingClientRect();
    const delta = rowRect.top - listRect.top - (listRect.height - rowRect.height) / 2;
    const top = Math.max(0, list.scrollTop + delta);
    if (instant) list.scrollTop = top;
    else list.scrollTo({ top, behavior: 'smooth' });
  }

  $effect(() => {
    const open = player.queueOpen;
    // 依赖正在播放的下标：面板开着时切歌也会跟着滚
    const index = player.index;
    if (!open) {
      wasOpen = false;
      return;
    }
    // 刚打开时直接到位；面板开着时换歌才用平滑滚动
    const instant = !wasOpen;
    wasOpen = true;
    // 等面板挂上、拿到真实高度之后再定位
    const raf = requestAnimationFrame(() => centerCurrent(instant));
    return () => cancelAnimationFrame(raf);
  });
</script>

{#if player.queueOpen}
  <div
    class="queue-mask"
    role="presentation"
    onclick={closeQueue}
    transition:fade={{ duration: 150 }}
  ></div>
  <div
    class="queue-panel"
    role="dialog"
    aria-label="播放列表"
    transition:fly={{ y: 10, duration: 180, opacity: 0 }}
  >
    <div class="qp-head">
      <span class="qp-title">播放列表 <em>{player.queue.length} 首</em></span>
      <div class="qp-actions">
        <button
          class="qp-icon"
          title="清空播放列表"
          aria-label="清空播放列表"
          disabled={player.queue.length === 0}
          onclick={clearQueue}
        >
          <HugeiconsIcon icon={icons.trash} size={ICON_SIZE.inline} />
        </button>
        <button class="qp-icon" title="收起" aria-label="收起播放列表" onclick={closeQueue}>
          <HugeiconsIcon icon={icons.chevronUp} size={ICON_SIZE.inline} />
        </button>
      </div>
    </div>


    {#if player.queue.length === 0}
      <div class="qp-empty">播放列表是空的 —— 在歌曲上点播放就会自动填充</div>
    {:else}
      <div class="qp-list" bind:this={listEl}>
        {#each player.queue as t, i (t.provider + t.id + i)}
          <div
            class="qp-row"
            class:on={i === player.index}
            role="group"
            oncontextmenu={(e) =>
              openTrackMenu(e, t, {
                queue: player.queue,
                extra: [
                  {
                    label: '从播放列表移除',
                    icon: icons.trash,
                    danger: true,
                    run: () => removeAt(i),
                  },
                ],
              })}
          >
            <button
              class="qp-item"
              onclick={() => jumpTo(i)}
              title="播放：{t.title}"
              aria-label="播放 {t.title}"
            >
              <span class="qp-i">
                {#if i === player.index}
                  {#if player.status === 'loading'}
                    <span class="spin"
                      ><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span
                    >
                  {:else if player.status === 'playing'}
                    <span class="bars"><i></i><i></i><i></i></span>
                  {:else}
                    <HugeiconsIcon icon={icons.pause} size={12} />
                  {/if}
                {:else}
                  {i + 1}
                {/if}
              </span>
              <span class="qp-t">
                <span class="qp-name">{t.title}</span>
                <span class="qp-artist">{t.artist || '未知艺人'}</span>
              </span>
              <span class="qp-d">{fmtDuration(t.duration_sec)}</span>
            </button>
            <button
              class="qp-x"
              title="从列表移除"
              aria-label="从列表移除 {t.title}"
              onclick={() => removeAt(i)}
            >
              <HugeiconsIcon icon={icons.close} size={12} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}
