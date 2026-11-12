<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, fmtBytes, qualityLabel, type DownloadRow, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { requestDownload } from '../lib/download.svelte';
  import { failureUi, loadFailures, openFailures } from '../lib/downloadFailures.svelte';
  import { copyText, showToast } from '../lib/toast.svelte';
  import DownloadsSkeleton from '../components/DownloadsSkeleton.svelte';
  import { delayedVisible } from '../lib/loading.svelte';

  let rows = $state<DownloadRow[]>([]);
  let loaded = $state(false);
  let err = $state('');
  let timer: number | undefined;
  let confirmClear = $state(false);
  let clearing = $state(false);

  const skel = delayedVisible(() => !loaded);

  async function refresh() {
    try {
      rows = await api.downloads();
    } catch (e) {
      err = String(e);
    } finally {
      loaded = true;
    }
  }

  $effect(() => {
    refresh();
    void loadFailures();
    timer = setInterval(refresh, 1500) as unknown as number;
    return () => clearInterval(timer);
  });

  async function doClear() {
    if (clearing) return;
    clearing = true;
    try {
      await api.downloadClear();
      rows = [];
      confirmClear = false;
      showToast('已清除下载记录');
    } catch (e) {
      err = String(e);
    } finally {
      clearing = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') confirmClear = false;
  }

  /** 下载记录里只有基础字段，重新下载时补成 Track */
  function rowTrack(r: DownloadRow): Track {
    return {
      provider: r.provider,
      id: r.id,
      title: r.title,
      artist: r.artist,
      artist_id: '',
      album: '',
      album_id: '',
      duration_sec: 0,
      cover: null,
      availability: 'unknown',
      subtitle: null,
    };
  }

  function rowMenu(e: MouseEvent, r: DownloadRow) {
    e.preventDefault();
    e.stopPropagation();
    const isS3 = r.path.startsWith('s3://');
    openMenuAt(e.clientX, e.clientY, [
      ...(isS3
        ? []
        : [
            {
              label: '打开下载目录',
              icon: icons.folder,
              run: () => api.openDownloadDir().catch(() => {}),
            },
          ]),
      {
        label: '复制文件路径',
        icon: icons.link,
        run: () => copyText(r.path || '记录里还没有落盘路径', '已复制路径'),
      },
      { sep: true },
      {
        label: '重新下载',
        icon: icons.download,
        run: () => requestDownload([rowTrack(r)]),
      },
    ]);
  }

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
    {#if failureUi.items.length > 0}
      <button class="mini warn" onclick={openFailures}>
        <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
        {failureUi.items.length} 首没下完整
      </button>
    {/if}
    {#if rows.length > 0}
      <button class="mini" onclick={() => (confirmClear = true)}>
        <HugeiconsIcon icon={icons.trash} size={ICON_SIZE.inline} />
        清除记录
      </button>
    {/if}
  </div>

  {#if err}<div class="err">{err}</div>{/if}

  {#if skel.visible}
    <DownloadsSkeleton rows={6} />
  {:else if !loaded}
    <!-- 等骨架屏延迟 -->
  {:else if rows.length === 0}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.download} size={ICON_SIZE.empty} />
      </div>
      <p>还没有下载记录</p>
    </div>
  {:else}
    <table class="dl">
      <thead>
        <tr><th>歌曲</th><th>艺人</th><th>音质</th><th>位置</th><th>大小</th><th>状态</th></tr>
      </thead>
      <tbody>
        {#each rows as r (r.task_id)}
          <tr oncontextmenu={(e) => rowMenu(e, r)}>
            <td>{r.title}</td>
            <td>{r.artist}</td>
            <td>{qualityLabel(r.quality)}</td>
            <td class="loc">
              {#if r.path.startsWith('s3://')}
                <span class="loc-tag cloud">网盘</span>
              {:else}
                <span class="loc-tag">本地</span>
              {/if}
            </td>
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

<svelte:window onkeydown={onKey} />

{#if confirmClear}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) confirmClear = false;
    }}
  >
    <div class="modal narrow" role="dialog" aria-modal="true" aria-label="清除下载记录">
      <div class="modal-head">
        <h2>清除下载记录</h2>
        <button class="icon" onclick={() => (confirmClear = false)} aria-label="关闭">
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>
      <p class="modal-note">
        只清空这份记录，不会删除已经下载到本地或网盘的文件。
      </p>
      <div class="modal-actions">
        <button class="mini" onclick={() => (confirmClear = false)} disabled={clearing}>取消</button>
        <button class="mini danger" onclick={doClear} disabled={clearing}>
          {clearing ? '清除中…' : '清除'}
        </button>
      </div>
    </div>
  </div>
{/if}
