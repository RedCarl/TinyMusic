<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import type { FailedDownload } from '../lib/api';
  import {
    failureUi,
    anyBusy,
    closeFailures,
    retryFailures,
    dismissFailure,
    dismissAllFailures,
  } from '../lib/downloadFailures.svelte';

  const items = $derived(failureUi.items);
  const busy = $derived(anyBusy());

  const PART_LABEL: Record<string, string> = {
    audio: '音频',
    lyrics: '歌词',
    cover: '封面',
  };

  function partLabel(part: string): string {
    return PART_LABEL[part] ?? part;
  }

  function targetLabel(f: FailedDownload): string {
    return f.target === 's3' ? '网盘' : '本地';
  }

  /** 一条记录里第一个错误，够用来判断原因 */
  function firstError(f: FailedDownload): string {
    return f.parts[0]?.error ?? '';
  }

  function isBusy(f: FailedDownload): boolean {
    return failureUi.busy.includes(f.id);
  }

  function close() {
    if (busy) return;
    closeFailures();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if failureUi.open && items.length}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label="下载失败的歌曲">
      <div class="modal-head">
        <div>
          <h2>{items.length} 首没下完整</h2>
          <p class="hint">
            音频 / 歌词 / 封面每项都自动重试过 5 次。可以只补失败的部分，成功后这条记录会自动消失。
          </p>
        </div>
        <button class="icon" onclick={close} aria-label="关闭" disabled={busy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      <div class="fail-list">
        {#each items as f (f.id)}
          <div class="fail-row" class:busy={isBusy(f)}>
            <div class="fail-main">
              <div class="fail-title">
                <span class="ft" title={f.track.title}>{f.track.title}</span>
                <span class="fa">{f.track.artist}</span>
                <span class="ttag">{targetLabel(f)}</span>
              </div>
              <div class="fail-parts">
                {#each f.parts as p}
                  <span class="part" class:audio={p.part === 'audio'} title={p.error}>
                    {partLabel(p.part)}
                  </span>
                {/each}
                {#if f.playlist}
                  <span class="pl">{f.playlist}</span>
                {/if}
              </div>
              {#if firstError(f)}
                <div class="fail-err" title={firstError(f)}>{firstError(f)}</div>
              {/if}
            </div>

            <div class="fail-side">
              <button
                class="mini"
                onclick={() => dismissFailure(f.id)}
                disabled={busy}
                title="不再提示，保留已经下好的部分"
              >
                忽略
              </button>
              <button
                class="mini primary"
                onclick={() => retryFailures([f.id])}
                disabled={busy}
              >
                {isBusy(f) ? '重试中…' : '重试'}
              </button>
            </div>
          </div>
        {/each}
      </div>

      {#if failureUi.error}
        <div class="err small fail-msg">{failureUi.error}</div>
      {:else if busy}
        <div class="ok small fail-msg">正在重试，失败的部分会在这里更新…</div>
      {/if}

      <div class="modal-actions">
        <button class="mini" onclick={dismissAllFailures} disabled={busy}>全部忽略</button>
        <button
          class="primary"
          onclick={() => retryFailures(items.map((f) => f.id))}
          disabled={busy}
        >
          <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
          {busy ? '重试中…' : '全部重试'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal {
    width: min(600px, 100%);
  }

  .fail-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 46vh;
    margin-top: 12px;
    padding: 0 18px;
    overflow: auto;
  }

  .fail-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--r-sm);
    background-color: var(--bg2);
    box-shadow: var(--pane-shadow);
    transition: opacity 160ms ease-out;
  }

  .fail-row.busy {
    opacity: 0.6;
  }

  .fail-main {
    flex: 1;
    min-width: 0;
  }

  .fail-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .ft {
    overflow: hidden;
    color: var(--fg);
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fa {
    flex: none;
    overflow: hidden;
    max-width: 40%;
    color: var(--fg3);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ttag {
    flex: none;
    padding: 1px 6px;
    border-radius: 999px;
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    color: var(--fg3);
    font-size: 10px;
  }

  .fail-parts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 5px;
    margin-top: 5px;
  }

  .part {
    padding: 1px 7px;
    border-radius: 999px;
    background-color: var(--warn-bg);
    box-shadow: inset 0 0 0 1px var(--warn-border);
    color: var(--warn);
    font-size: 10.5px;
  }

  .part.audio {
    color: var(--danger);
  }

  .pl {
    overflow: hidden;
    max-width: 46%;
    color: var(--fg3);
    font-size: 10.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fail-err {
    margin-top: 4px;
    overflow: hidden;
    color: var(--fg3);
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fail-side {
    display: flex;
    flex: none;
    gap: 6px;
  }

  .fail-msg {
    padding: 10px 18px 0;
  }
</style>
