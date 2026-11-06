<script lang="ts">
  import TrackListSkeleton from './TrackListSkeleton.svelte';

  /** 专辑 / 歌单详情页：头部（封面 + 信息）+ 批量操作 + 曲目列表 */
  let {
    rows = 10,
    description = false
  }: {
    rows?: number;
    description?: boolean;
  } = $props();
</script>

<div class="sk-block" role="status" aria-label="加载中">
  <div class="head sk-detail-head">
    <span class="sk sk-detail-cover"></span>
    <div class="info sk-detail-info">
      <span class="sk sk-h1"></span>
      <span class="sk sk-subline"></span>
      {#if description}
        <span class="sk sk-desc-line"></span>
        <span class="sk sk-desc-line short"></span>
      {/if}
    </div>
  </div>

  <div class="batchbar sk-batchbar">
    <span class="sk sk-btn"></span>
    <span class="sk sk-btn ghost"></span>
  </div>

  <TrackListSkeleton {rows} status={false} />
</div>
