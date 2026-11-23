<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { openTrackMenu } from '../lib/trackMenu.svelte';
  import { appCfg } from '../lib/config.svelte';
  import { isFavorite, primeFavorites, toggleFavorite } from '../lib/favorite.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { t } from '../lib/i18n.svelte';
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
    // 面板打开时一次标好红心状态（只查一次收藏清单）
    if (appCfg.s3Enabled) void primeFavorites(player.queue);
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
    aria-label={t('播放列表')}
    transition:fly={{ y: 10, duration: 180, opacity: 0 }}
  >
    <div class="qp-head">
      <span class="qp-title">{t('播放列表')} <em>{t('{n} 首', { n: player.queue.length })}</em></span>
      <div class="qp-actions">
        <button
          class="qp-icon"
          title={t('清空播放列表')}
          aria-label={t('清空播放列表')}
          disabled={player.queue.length === 0}
          onclick={clearQueue}
        >
          <HugeiconsIcon icon={icons.trash} size={ICON_SIZE.inline} />
        </button>
        <button class="qp-icon" title={t('收起')} aria-label={t('收起播放列表')} onclick={closeQueue}>
          <HugeiconsIcon icon={icons.chevronUp} size={ICON_SIZE.inline} />
        </button>
      </div>
    </div>


    {#if player.queue.length === 0}
      <div class="qp-empty">{t('播放列表是空的 —— 在歌曲上点播放就会自动填充')}</div>
    {:else}
      <div class="qp-list" bind:this={listEl}>
        {#each player.queue as track, i (track.provider + track.id + i)}
          <div
            class="qp-row"
            class:on={i === player.index}
            role="group"
            oncontextmenu={(e) =>
              openTrackMenu(e, track, {
                queue: player.queue,
                extra: [
                  {
                    label: t('从播放列表移除'),
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
              title={t('播放：{title}', { title: track.title })}
              aria-label={t('播放 {title}', { title: track.title })}
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
                <span class="qp-name">
                  {track.title}
                  {#if track.available_qualities?.includes('lossless')}
                    <span class="badge sq">{t('无损')}</span>
                  {/if}
                </span>
                <span class="qp-artist">{track.artist || t('未知艺人')}</span>
              </span>
              <span class="qp-d">{fmtDuration(track.duration_sec)}</span>
            </button>
            {#if appCfg.s3Enabled}
              <button
                class="qp-fav"
                class:on={isFavorite(track)}
                title={isFavorite(track) ? t('取消喜欢') : t('喜欢（收藏到网盘）')}
                aria-label={isFavorite(track) ? t('取消喜欢') : t('喜欢')}
                aria-pressed={isFavorite(track)}
                onclick={() =>
                  toggleFavorite(track).catch((e) => showToast(String(e)))}
              >
                <HugeiconsIcon icon={icons.favorite} size={12} />
              </button>
            {/if}
            <button
              class="qp-x"
              title={t('从列表移除')}
              aria-label={t('从列表移除 {title}', { title: track.title })}
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
