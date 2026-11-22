<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { api, type FavoriteDuplicate } from '../lib/api';
  import ErrorNotice from './ErrorNotice.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { t } from '../lib/i18n.svelte';

  let { open = $bindable(false) } = $props();

  let items = $state<FavoriteDuplicate[]>([]);
  let loading = $state(false);
  let busy = $state(false);
  let error = $state('');
  let scanned = $state(false);

  const totalBytes = $derived(items.reduce((sum, d) => sum + d.bytes, 0));

  function prettyBytes(n: number): string {
    if (n >= 1048576) return (n / 1048576).toFixed(1) + ' MB';
    if (n >= 1024) return (n / 1024).toFixed(0) + ' KB';
    return n + ' B';
  }

  function fileName(key: string): string {
    return key.split('/').pop() || key;
  }

  async function scan() {
    loading = true;
    error = '';
    items = [];
    scanned = false;
    try {
      items = await api.favoritesDedupeScan();
      scanned = true;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  // 每次打开都重新扫一遍，别拿旧结果去删
  $effect(() => {
    if (open) void scan();
  });

  async function purge() {
    if (busy || items.length === 0) return;
    busy = true;
    error = '';
    try {
      const freed = await api.favoritesDedupePurge(items.map((d) => d.duplicate));
      showToast(t('已清理 {n} 项，释放 {size}', { n: items.length, size: prettyBytes(freed) }));
      open = false;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function close() {
    if (!busy) open = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label={t('清理喜欢重复副本')}>
      <div class="modal-head">
        <div>
          <h2>{t('清理喜欢重复副本')}</h2>
          <p class="hint">{t('只删「喜欢」目录里那份，歌单里的原文件不动')}</p>
        </div>
        <button class="icon" onclick={close} aria-label={t('关闭')} disabled={busy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      {#if error}<div class="pad"><ErrorNotice text={error} compact /></div>{/if}

      {#if loading}
        <div class="pad loading">
          <HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} />
          {t('正在扫描网盘…')}
        </div>
      {:else if items.length === 0}
        <div class="pad empty">
          <strong>{scanned ? t('没有发现重复副本') : t('还没有扫描')}</strong>
          <p>{t('同一首歌在歌单和「喜欢」目录各有一份时，才能清掉多出来的那份')}</p>
        </div>
      {:else}
        <div class="summary">{t('共 {n} 项，占用 {size}', { n: items.length, size: prettyBytes(totalBytes) })}</div>
        <div class="dup-list">
          {#each items as item (item.duplicate)}
            <div class="dup">
              <div class="dup-main">
                <span class="dup-name">{fileName(item.duplicate)}</span>
                <span class="dup-path">{item.duplicate}</span>
              </div>
              <span class="dup-size">{prettyBytes(item.bytes)}</span>
            </div>
          {/each}
        </div>
      {/if}

      <div class="modal-actions">
        <button class="mini" onclick={close} disabled={busy}>{t('取消')}</button>
        <button
          class="mini danger"
          onclick={purge}
          disabled={busy || loading || items.length === 0}
        >
          {busy ? t('处理中…') : t('删除选中')}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal {
    width: min(620px, 100%);
  }

  .pad {
    padding: 10px 18px 0;
  }

  .loading {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--fg2);
    font-size: 13px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 26px 18px 10px;
    text-align: center;
  }

  .empty strong {
    font-size: 13.5px;
    font-weight: 600;
  }

  .empty p {
    max-width: 400px;
    margin: 0;
    color: var(--fg3);
    font-size: 11.5px;
    line-height: 1.6;
  }

  .summary {
    padding: 12px 18px 0;
    color: var(--fg2);
    font-size: 12.5px;
  }

  .dup-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 42vh;
    overflow-y: auto;
    padding: 10px 18px 0;
  }

  .dup {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 10px;
    background-color: var(--glass-control);
  }

  .dup-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .dup-name {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--fg);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dup-path {
    font-size: 11px;
    color: var(--fg3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dup-size {
    font-size: 11.5px;
    color: var(--fg3);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
</style>
