<script lang="ts">
  let { count = 6 }: { count?: number } = $props();
</script>

<div class="skeleton-cards" aria-hidden="true">
  {#each Array(count) as _, i (i)}
    <div class="sk-card">
      <span class="sk sk-square"></span>
      <span class="sk sk-line w1"></span>
    </div>
  {/each}
</div>
