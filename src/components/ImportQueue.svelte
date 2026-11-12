<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons } from '../lib/icons';
  import {
    importQueue,
    removeImport,
    clearFinishedImports,
  } from '../lib/importQueue.svelte';
  import ProgressBar from './ProgressBar.svelte';
  import ErrorNotice from './ErrorNotice.svelte';

  const queuedCount = $derived(
    importQueue.tasks.filter((t) => t.status === 'queued').length
  );
  const finishedCount = $derived(
    importQueue.tasks.filter((t) => t.status === 'done' || t.status === 'error').length
  );
</script>

{#if importQueue.tasks.length}
  <section class="impq" aria-label="导入队列">
    <div class="impq-head">
      <span class="impq-title">导入队列</span>
      <span class="impq-count">
        {#if importQueue.running}导入中{/if}
        {#if queuedCount}
          {#if importQueue.running} · {/if}{queuedCount} 个排队
        {/if}
      </span>
      {#if finishedCount}
        <button class="impq-clear" onclick={clearFinishedImports}>清空已结束</button>
      {/if}
    </div>

    <div class="impq-list">
      {#each importQueue.tasks as t (t.id)}
        <div class="impq-row">
          <div class="impq-main">
            <div class="impq-names" title={`${t.name} → ${t.target}`}>
              <span class="impq-src">{t.name}</span>
              <span class="impq-arrow">→</span>
              <span class="impq-target">{t.target}</span>
            </div>
            {#if t.status === 'running'}
              <ProgressBar value={t.pct} label={t.label || '正在导入…'} />
            {:else if t.status === 'queued'}
              <div class="impq-sub">排队中，轮到后自动开始</div>
            {:else if t.status === 'done'}
              <div class="impq-sub ok">
                已导入 {t.uploaded} 首{#if t.failed}，{t.failed} 首失败{/if}
              </div>
            {:else}
              <ErrorNotice text={t.error || '导入失败'} compact />
            {/if}
          </div>

          <div class="impq-side">
            {#if t.status === 'running'}
              <span class="impq-badge on">导入中</span>
            {:else if t.status === 'queued'}
              <span class="impq-badge">排队中</span>
            {:else if t.status === 'done'}
              <span class="impq-badge done">完成</span>
            {:else}
              <span class="impq-badge bad">失败</span>
            {/if}
            {#if t.status !== 'running'}
              <button
                class="impq-x"
                title="移除任务"
                aria-label={`移除 ${t.name} 的导入任务`}
                onclick={() => removeImport(t.id)}
              >
                <HugeiconsIcon icon={icons.close} size={12} />
              </button>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  </section>
{/if}
