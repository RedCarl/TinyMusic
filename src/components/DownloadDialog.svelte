<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import {
    downloadUi,
    closeDownloadDialog,
    createPlaylistAndSelect,
    runLocal,
    runS3,
  } from '../lib/download.svelte';

  const count = $derived(downloadUi.targets.length);
  const preview = $derived(downloadUi.targets.slice(0, 4));
  const more = $derived(Math.max(0, count - preview.length));

  function close() {
    if (downloadUi.busy) return;
    closeDownloadDialog(true);
  }

  async function start() {
    if (downloadUi.busy) return;
    downloadUi.err = '';
    if (downloadUi.mode === 's3') {
      let pl = downloadUi.playlist;
      if (downloadUi.newPlaylist.trim()) {
        pl = downloadUi.newPlaylist.trim();
        await createPlaylistAndSelect(pl);
        if (downloadUi.err) return;
        pl = downloadUi.playlist || pl;
      }
      await runS3(downloadUi.targets, pl);
    } else {
      await runLocal(downloadUi.targets);
    }
    if (!downloadUi.err) {
      setTimeout(() => {
        if (!downloadUi.busy) downloadUi.open = false;
      }, 900);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if downloadUi.open}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label="下载歌曲">
      <div class="modal-head">
        <div>
          <h2>下载 {count} 首歌曲</h2>
          <p class="hint">音频内嵌封面标签，歌词保存为同名 .lrc 文件</p>
        </div>
        <button class="icon" onclick={close} aria-label="关闭" disabled={downloadUi.busy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      <div class="targets">
        <label class="target" class:on={downloadUi.mode === 'local'}>
          <input type="radio" bind:group={downloadUi.mode} value="local" />
          <span class="tico"><HugeiconsIcon icon={icons.downloadDir} size={ICON_SIZE.inline} /></span>
          <span>
            <strong>下载到本地</strong>
            <em>保存到设置里的下载目录</em>
          </span>
        </label>
        <label class="target" class:on={downloadUi.mode === 's3'}>
          <input type="radio" bind:group={downloadUi.mode} value="s3" />
          <span class="tico"><HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} /></span>
          <span>
            <strong>下载到网盘</strong>
            <em>S3 兼容存储，可归入歌单</em>
          </span>
        </label>
      </div>

      {#if downloadUi.mode === 's3'}
        <div class="field">
          <div class="lbl">网盘歌单</div>
          <select class="select" bind:value={downloadUi.playlist} disabled={downloadUi.busy}>
            <option value="">（未分类，放在根目录）</option>
            {#each downloadUi.playlists as p (p.name)}
              <option value={p.name}>{p.name}（{p.count} 首）</option>
            {/each}
          </select>
          <div class="newpl">
            <input
              bind:value={downloadUi.newPlaylist}
              placeholder="新建歌单名称（可选）"
              disabled={downloadUi.busy}
            />
            <button
              class="mini"
              disabled={downloadUi.busy || !downloadUi.newPlaylist.trim()}
              onclick={() => createPlaylistAndSelect(downloadUi.newPlaylist)}
            >
              <HugeiconsIcon icon={icons.plus} size={ICON_SIZE.inline} />
              新建歌单
            </button>
          </div>
        </div>
      {/if}

      <div class="dl-preview">
        {#each preview as t (t.provider + t.id)}
          <div class="dl-prow">
            <span class="dp-t">{t.title}</span>
            <span class="dp-a">{t.artist}</span>
            <span class="dp-d">{fmtDuration(t.duration_sec)}</span>
          </div>
        {/each}
        {#if more > 0}
          <div class="dl-prow muted">… 还有 {more} 首</div>
        {/if}
      </div>

      {#if downloadUi.busy || downloadUi.progress}
        <div class="modal-progress">
          <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
          {downloadUi.progress || '处理中…'}
        </div>
      {/if}
      {#if downloadUi.doneMsg}<div class="ok small">{downloadUi.doneMsg}</div>{/if}
      {#if downloadUi.err}<div class="err small">{downloadUi.err}</div>{/if}

      <div class="modal-actions">
        <button class="mini" onclick={close} disabled={downloadUi.busy}>取消</button>
        <button class="primary" onclick={start} disabled={downloadUi.busy || count === 0}>
          <HugeiconsIcon
            icon={downloadUi.mode === 's3' ? icons.cloud : icons.download}
            size={ICON_SIZE.inline}
          />
          {downloadUi.busy ? '任务进行中…' : downloadUi.mode === 's3' ? '下载到网盘' : '下载到本地'}
        </button>
      </div>
    </div>
  </div>
{/if}
