<script lang="ts">
  import { toast, copyText } from '../lib/toast.svelte';
</script>

{#if toast.visible}
  <div
    class="toast"
    role="status"
    aria-live="polite"
    title="点击复制这条提示"
    onclick={() => copyText(toast.msg, '已复制')}
  >
    {toast.msg}
  </div>
{/if}
