<script lang="ts">
  import { toast } from '../lib/toast.svelte';
</script>

{#if toast.visible}
  <div class="toast" role="status" aria-live="polite">{toast.msg}</div>
{/if}
