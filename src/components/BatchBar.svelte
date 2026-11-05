<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { playTrack, player } from '../lib/player.svelte';

  let { tracks, label = '' }: { tracks: Track[]; label?: string } = $props();

  let busy = $state(false);
  let msg = $state('');

  async function downloadAll() {
    if (busy || tracks.length === 0) return;
    busy = true;
    msg = '准备中…';
    try {
      const ids = await api.downloadBatch(tracks, player.quality, (p) => {
        if (p.total && p.task_id.startsWith('batch-') && !p.task_id.startsWith('batch-done')) {
          msg = `第 ${p.downloaded + 1} / ${p.total} 首`;
        }
      });
      msg = `已下载 ${ids.length} 首`;
    } catch (e) {
      msg = '批量下载失败';
      player.error = String(e);
    } finally {
      busy = false;
      setTimeout(() => (msg = ''), 4000);
    }
  }
</script>

<div class="batchbar">
  <button
    class="primary"
    disabled={tracks.length === 0}
    onclick={() => tracks[0] && playTrack(tracks[0], tracks)}
  >
    <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
    播放全部
  </button>
  <button disabled={busy || tracks.length === 0} onclick={downloadAll}>
    <HugeiconsIcon icon={icons.download} size={ICON_SIZE.inline} />
    {busy ? '下载中…' : `下载全部${label ? ' · ' + label : ''}`}
  </button>
  {#if msg}<span class="muted small">{msg}</span>{/if}
</div>
