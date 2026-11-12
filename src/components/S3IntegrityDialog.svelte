<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import type { S3IntegrityIssue } from '../lib/api';
  import ProgressBar from './ProgressBar.svelte';
  import {
    integrityUi,
    integrityBusy,
    fixableIssues,
    closeIntegrity,
    runIntegrityCheck,
    fixIntegrity,
    issueStatus,
    issueError,
  } from '../lib/s3Integrity.svelte';
  import ErrorNotice from './ErrorNotice.svelte';

  const issues = $derived(integrityUi.issues);
  const fixed = $derived(integrityUi.fixed);
  const busy = $derived(integrityBusy());
  const scope = $derived(integrityUi.playlist || '全部歌单');
  const fixable = $derived(fixableIssues().length);
  const failedCount = $derived(
    issues.filter((i) => issueStatus(i.id) === 'failed').length
  );
  const rechecking = $derived(
    integrityUi.runActive &&
      integrityUi.runTotal > 0 &&
      integrityUi.runDone >= integrityUi.runTotal
  );

  const PART_LABEL: Record<string, string> = {
    audio: '音频',
    lyrics: '歌词',
    cover: '封面',
  };

  function partLabel(part: string): string {
    return PART_LABEL[part] ?? part;
  }

  function displayTitle(issue: S3IntegrityIssue): string {
    return issue.title || issue.stem.split('/').pop() || '未命名';
  }

  function actionLabel(issue: S3IntegrityIssue): string {
    switch (issueStatus(issue.id)) {
      case 'running':
        return '补齐中…';
      case 'waiting':
        return busy ? '等待中' : '补齐';
      case 'failed':
        return '重试';
      default:
        return '补齐';
    }
  }

  function actionDisabled(issue: S3IntegrityIssue): boolean {
    const st = issueStatus(issue.id);
    return busy || !issue.can_fix || st === 'running' || (st === 'waiting' && busy);
  }

  function progressLabel(): string {
    if (rechecking) return '正在重新检查…';
    const current = Math.min(integrityUi.runDone + 1, integrityUi.runTotal);
    return `正在补齐 ${current} / ${integrityUi.runTotal}`;
  }

  function close() {
    if (busy) return;
    closeIntegrity();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') close();
  }
</script>

<svelte:window onkeydown={onKey} />

{#if integrityUi.open}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label="网盘完整性检查">
      <div class="modal-head">
        <div>
          <h2>网盘完整性检查</h2>
          <p class="hint">{scope} · 检查每首歌的音频、歌词、封面是否齐全</p>
        </div>
        <button class="icon" onclick={close} aria-label="关闭" disabled={busy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      {#if integrityUi.error}
        <div class="pad"><ErrorNotice text={integrityUi.error} compact /></div>
      {/if}

      {#if integrityUi.loading}
        <div class="pad loading">
          <HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} />
          正在扫描网盘…
        </div>
      {:else if issues.length === 0 && fixed.length === 0}
        <div class="pad empty">
          <span class="empty-ico">
            <HugeiconsIcon icon={icons.shield} size={ICON_SIZE.control} />
          </span>
          <strong>
            {integrityUi.checked > 0
              ? `已检查 ${integrityUi.checked} 首，内容齐全`
              : '没有扫描到歌曲'}
          </strong>
          <p>缺音频、歌词或封面的条目会出现在这里，可以逐个补齐。</p>
        </div>
      {:else}
        <div class="summary">
          已检查 {integrityUi.checked} 首，<b>{issues.length}</b> 首缺内容
          {#if fixed.length > 0}
            <span class="fixed-note">· 已补齐 {fixed.length} 首</span>
          {/if}
          {#if failedCount > 0}
            <span class="failed-note">· {failedCount} 首补齐失败</span>
          {/if}
        </div>

        {#if busy}
          <ProgressBar
            value={rechecking
              ? null
              : integrityUi.runTotal > 0
                ? integrityUi.runDone / integrityUi.runTotal
                : null}
            label={progressLabel()}
          />
        {/if}

        {#if fixed.length > 0}
          <div class="block-title ok">已补齐</div>
          <div class="issue-list fixed-list">
            {#each fixed as issue (issue.id)}
              <div class="issue-row done">
                <div class="issue-main">
                  <div class="issue-title">
                    <span class="it" title={displayTitle(issue)}>{displayTitle(issue)}</span>
                    {#if issue.artist}<span class="ia">{issue.artist}</span>{/if}
                    {#if issue.playlist}
                      <span class="ttag" title={issue.playlist}>{issue.playlist}</span>
                    {/if}
                  </div>
                  <div class="issue-parts">
                    {#each issue.missing as part}
                      <span class="part done">{partLabel(part)}</span>
                    {/each}
                  </div>
                </div>
                <span class="state-chip ok">已补齐</span>
              </div>
            {/each}
          </div>
        {/if}

        {#if issues.length > 0}
          <div class="block-title">待补齐</div>
          <div class="issue-list">
            {#each issues as issue (issue.id)}
              {@const st = issueStatus(issue.id)}
              <div
                class="issue-row"
                class:running={st === 'running'}
                class:waiting={st === 'waiting'}
                class:failed={st === 'failed'}
              >
                <div class="issue-main">
                  <div class="issue-title">
                    <span class="it" title={displayTitle(issue)}>{displayTitle(issue)}</span>
                    {#if issue.artist}<span class="ia">{issue.artist}</span>{/if}
                    {#if issue.playlist}
                      <span class="ttag" title={issue.playlist}>{issue.playlist}</span>
                    {/if}
                  </div>
                  <div class="issue-parts">
                    {#each issue.missing as part}
                      <span class="part" class:audio={part === 'audio'}>
                        {partLabel(part)}缺失
                      </span>
                    {/each}
                    {#if st === 'running'}
                      <span class="state-chip run">正在补齐</span>
                    {:else if st === 'waiting' && busy}
                      <span class="state-chip wait">等待中</span>
                    {:else if st === 'failed'}
                      <span class="state-chip fail">失败</span>
                    {/if}
                    {#if !issue.can_fix}
                      <span class="nofix" title="没有这台机器的上传记录，找不到原平台信息">
                        无来源
                      </span>
                    {/if}
                  </div>
                  {#if st === 'failed' && issueError(issue.id)}
                    <ErrorNotice text={issueError(issue.id)} compact />
                  {/if}
                </div>
                <button
                  class="mini primary"
                  onclick={() => fixIntegrity([issue.id])}
                  disabled={actionDisabled(issue)}
                >
                  {actionLabel(issue)}
                </button>
              </div>
            {/each}
          </div>
        {/if}
      {/if}

      <div class="modal-actions">
        <button class="mini" onclick={runIntegrityCheck} disabled={busy || integrityUi.loading}>
          重新检查
        </button>
        <button
          class="mini primary"
          onclick={() => fixIntegrity(issues.map((i) => i.id))}
          disabled={busy || fixable === 0}
        >
          <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
          全部补齐
        </button>
        <button class="primary" onclick={close} disabled={busy}>关闭</button>
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

  .empty-ico {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    margin-bottom: 4px;
    border-radius: 50%;
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    color: var(--success);
  }

  .empty strong {
    font-size: 13.5px;
    font-weight: 600;
  }

  .empty p {
    max-width: 380px;
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

  .summary b {
    color: var(--danger);
    font-weight: 600;
  }

  .fixed-note {
    color: var(--success);
  }

  .failed-note {
    color: var(--danger);
  }

  .block-title {
    padding: 14px 18px 0;
    color: var(--fg3);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
  }

  .block-title.ok {
    color: var(--success);
  }

  .issue-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 34vh;
    margin-top: 8px;
    padding: 0 18px;
    overflow: auto;
  }

  .fixed-list {
    max-height: 22vh;
  }

  .issue-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border-radius: var(--r-sm);
    background-color: var(--bg2);
    box-shadow: var(--pane-shadow);
    transition:
      opacity 160ms ease-out,
      box-shadow 160ms ease-out;
  }

  .issue-row.done {
    opacity: 0.75;
  }

  .issue-row.waiting {
    opacity: 0.6;
  }

  .issue-row.running {
    box-shadow:
      var(--pane-shadow),
      inset 0 0 0 1px var(--glass-state-rim);
  }

  .issue-row.failed {
    box-shadow:
      var(--pane-shadow),
      inset 0 0 0 1px color-mix(in oklch, var(--danger) 30%, transparent);
  }

  .issue-main {
    flex: 1;
    min-width: 0;
  }

  .issue-title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .it {
    overflow: hidden;
    color: var(--fg);
    font-size: 13px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ia {
    flex: none;
    overflow: hidden;
    max-width: 34%;
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

  .issue-parts {
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

  .part.done {
    background-color: var(--ok-bg);
    box-shadow: inset 0 0 0 1px var(--ok-border);
    color: var(--success);
  }

  .state-chip {
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 10.5px;
  }

  .state-chip.ok {
    background-color: var(--ok-bg);
    box-shadow: inset 0 0 0 1px var(--ok-border);
    color: var(--success);
  }

  .state-chip.run {
    background-color: var(--glass-state);
    color: var(--fg2);
  }

  .state-chip.wait {
    color: var(--fg3);
  }

  .state-chip.fail {
    background-color: color-mix(in oklch, var(--danger) 12%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in oklch, var(--danger) 26%, transparent);
    color: var(--danger);
  }

  .nofix {
    color: var(--fg3);
    font-size: 10.5px;
  }

</style>
