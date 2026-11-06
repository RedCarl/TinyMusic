<script lang="ts">
  /**
   * 与真实曲目列表（TrackRow 的 .row）同一套网格：
   * 直接把骨架放在 .row 的列里，窗口变窄时和真实行一样收起专辑/热度列。
   */
  let {
    rows = 8,
    toolbar = 'none',
    status = true
  }: {
    rows?: number;
    toolbar?: 'none' | 'search' | 'library';
    status?: boolean;
  } = $props();

  const titleWidths = [68, 52, 74, 46, 62, 57, 70, 48];
</script>

<div
  class="sk-block"
  role={status ? 'status' : undefined}
  aria-label={status ? '加载中' : undefined}
  aria-hidden={status ? undefined : 'true'}
>
  {#if toolbar === 'search'}
    <div class="summary sk-summary">
      <span class="sk sk-summary-line"></span>
    </div>
  {/if}

  {#if toolbar !== 'none'}
    <div class="batchbar sk-batchbar">
      <span class="sk sk-btn"></span>
      <span class="sk sk-btn ghost"></span>
    </div>
  {/if}

  <div class="list">
    {#each Array(rows) as _, i (i)}
      <div class="row sk-track-row">
        <span class="idx"><span class="sk sk-idxline"></span></span>
        <span class="play"><span class="sk sk-playdot"></span></span>
        <span class="title">
          <span class="sk sk-titleline" style="width:{titleWidths[i % titleWidths.length]}%"></span>
        </span>
        <span class="artist"><span class="sk sk-artistline"></span></span>
        <span class="album"><span class="sk sk-albumline"></span></span>
        <span class="heat"><span class="sk sk-heatline"></span></span>
        <span class="dur"><span class="sk sk-durline"></span></span>
        <span class="ops">
          <span class="sk sk-opsdot"></span>
          <span class="sk sk-opsdot"></span>
        </span>
      </div>
    {/each}
  </div>
</div>
