<script lang="ts">
  let { rows = 8, thumb = true }: { rows?: number; thumb?: boolean } = $props();
</script>

<div class="skeleton-list" aria-hidden="true">
  {#each Array(rows) as _, i (i)}
    <div class="sk-row">
      <span class="sk sk-idx"></span>
      {#if thumb}<span class="sk sk-cover"></span>{/if}
      <span class="sk-lines">
        <span class="sk sk-line w1"></span>
        <span class="sk sk-line w2"></span>
      </span>
      <span class="sk sk-dur"></span>
    </div>
  {/each}
</div>
