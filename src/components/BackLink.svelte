<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { router, back } from '../lib/router.svelte';

  const show = $derived(router.history.length > 0);
</script>

{#if show}
  <button class="backlink" onclick={() => back()} title="返回上一级" aria-label="返回上一级">
    <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
    返回
  </button>
{/if}
