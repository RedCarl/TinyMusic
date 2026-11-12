<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons } from '../lib/icons';
  import { copyText } from '../lib/toast.svelte';

  /**
   * 错误提示：自动换行，长报错可「展开」看全文，文字可选中；
   * 「复制」永远复制完整原文，方便贴到别处排查。
   */
  let {
    text,
    compact = false,
    class: klass = ''
  }: { text: string; compact?: boolean; class?: string } = $props();

  /** 长报错默认收起成几行，展开后完整显示（复制永远拿全文） */
  let expanded = $state(false);
  const long = $derived(text.length > 140 || text.split('\n').length > 3);

  // 换了一条报错（重试 / 换歌）就收回去
  $effect(() => {
    text;
    expanded = false;
  });

  async function copy() {
    await copyText(text.trim(), '已复制错误信息');
  }
</script>

<div class="errbox {klass}" class:compact role="alert">
  <div class="errtext" class:expanded title={text}>{text}</div>
  <div class="errside">
    {#if long}
      <button
        class="errcopy"
        type="button"
        onclick={() => (expanded = !expanded)}
        title={expanded ? '收起错误信息' : '展开完整错误信息'}
      >
        {expanded ? '收起' : '展开'}
      </button>
    {/if}
    <button
      class="errcopy"
      type="button"
      onclick={copy}
      title="复制完整错误信息"
      aria-label="复制完整错误信息"
    >
      <HugeiconsIcon icon={icons.copy} size={12} />
      <span>复制</span>
    </button>
  </div>
</div>

<style>
  .errbox {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 0 10px;
    color: var(--danger);
    font-size: 12.5px;
    line-height: 1.55;
  }

  .errbox.compact {
    gap: 8px;
    padding: 4px 0 0;
    font-size: 11.5px;
  }

  .errbox.compact .errtext {
    max-height: 4.6em;
  }

  .errtext {
    flex: 1;
    min-width: 0;
    max-height: 4.6em;
    overflow: hidden;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }

  .errtext.expanded {
    max-height: none;
    overflow: visible;
  }

  .errside {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .errcopy {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 22px;
    padding: 0 8px;
    border: 0;
    border-radius: 999px;
    color: var(--fg2);
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    font-size: 10.5px;
    line-height: 1;
    cursor: pointer;
    transition:
      background-color 140ms ease-out,
      color 140ms ease-out;
  }

  .errcopy:hover {
    color: var(--fg);
    background-color: var(--glass-state);
  }
</style>
