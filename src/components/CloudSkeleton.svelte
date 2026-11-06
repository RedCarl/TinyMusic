<script lang="ts">
  /** 网盘页两种骨架：歌单列表 / 歌单详情曲目 */
  let {
    mode = 'list',
    rows = 6
  }: {
    mode?: 'list' | 'detail';
    rows?: number;
  } = $props();
</script>

<div class="sk-block" role="status" aria-label="加载中">
  {#if mode === 'list'}
    <div class="plrows">
      {#each Array(4) as _, i (i)}
        <div class="plrow sk-cloud-row">
          <span class="plgrip"><span class="sk sk-grip"></span></span>
          <span class="plart"><span class="sk sk-fill"></span></span>
          <span class="pltext">
            <span class="sk sk-plname"></span>
            <span class="sk sk-plsub"></span>
          </span>
          <span class="placts">
            <span class="sk sk-act"></span>
            <span class="sk sk-act"></span>
            <span class="sk sk-act"></span>
          </span>
        </div>
      {/each}
    </div>
  {:else}
    <div class="s3list">
      {#each Array(rows) as _, i (i)}
        <div class="s3row sk-s3row">
          <span class="idx"><span class="sk sk-idxline"></span></span>
          <span class="s3cover sk-s3cover"><span class="sk sk-fill"></span></span>
          <span class="s3meta">
            <span class="sk sk-name"></span>
            <span class="sk sk-sub"></span>
          </span>
          <span class="playbtn sk-playbtn"><span class="sk sk-fill"></span></span>
        </div>
      {/each}
    </div>
  {/if}
</div>
