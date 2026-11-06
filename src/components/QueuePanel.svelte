<script lang="ts">
  import { fly } from 'svelte/transition';
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
</script>

{#if player.queueOpen}
  <div class="queue-mask" role="presentation" onclick={closeQueue}></div>
  <div
    class="queue-panel"
    role="dialog"
    aria-label="播放列表"
    transition:fly={{ y: 14, duration: 200, opacity: 0 }}
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
      <div class="qp-list">
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
