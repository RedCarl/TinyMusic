<script lang="ts">
  /** 账号歌单页：横向歌单行（名称 + 首数 [+ 导入按钮]） */
  let {
    rows = 6,
    withIcon = false
  }: {
    rows?: number;
    withIcon?: boolean;
  } = $props();
</script>

<div class="sk-block" role="status" aria-label="加载中">
  <div class="pllist wide">
    {#each Array(rows) as _, i (i)}
      <div class="plrow sk-plrow">
        <span class="plopen">
          <span class="sk sk-plname"></span>
          <span class="sk sk-plnum"></span>
        </span>
        {#if withIcon}
          <span class="plicon"><span class="sk sk-fill"></span></span>
        {/if}
      </div>
    {/each}
  </div>
</div>
