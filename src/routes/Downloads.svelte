<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, fmtBytes, type DownloadRow } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';

  let rows = $state<DownloadRow[]>([]);
  let err = $state('');
  let timer: number | undefined;

  async function refresh() {
    try {
      rows = await api.downloads();
    } catch (e) {
      err = String(e);
    }
  }

  $effect(() => {
    refresh();
    timer = setInterval(refresh, 1500) as unknown as number;
    return () => clearInterval(timer);
  });

  function statusLabel(s: string): string {
    if (s === 'done') return '已完成';
    if (s === 'running') return '进行中';
    if (s.startsWith('failed')) return '失败';
    return s;
  }
</script>

<div class="page">
  <div class="summary">
    下载记录
    <button class="mini" onclick={() => api.openDownloadDir().catch(() => {})}>
      <HugeiconsIcon icon={icons.folder} size={ICON_SIZE.inline} />
      打开目录
    </button>
  </div>

  {#if err}<div class="err">{err}</div>{/if}

  {#if rows.length === 0}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.download} size={ICON_SIZE.empty} />
      </div>
      <p>还没有下载记录</p>
    </div>
  {:else}
    <table class="dl">
      <thead>
        <tr><th>歌曲</th><th>艺人</th><th>音质</th><th>大小</th><th>状态</th></tr>
      </thead>
      <tbody>
        {#each rows as r (r.task_id)}
          <tr>
            <td>{r.title}</td>
            <td>{r.artist}</td>
            <td>{r.quality}</td>
            <td>{fmtBytes(r.bytes)}</td>
            <td class:ok={r.status === 'done'} class:bad={r.status.startsWith('failed')}>
              {statusLabel(r.status)}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
