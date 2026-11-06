<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, fmtBytes, qualityLabel, type DownloadRow, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { requestDownload } from '../lib/download.svelte';
  import { copyText } from '../lib/toast.svelte';

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
    <span class="hint">每首歌都会内嵌封面，并生成同名 .lrc 歌词文件</span>
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
