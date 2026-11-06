<script lang="ts">
  import TrackListSkeleton from './TrackListSkeleton.svelte';

  /** 艺人页：圆形头像 + 资料 + 简介 + 标签页 + 曲目列表 */
  let { rows = 8 }: { rows?: number } = $props();
</script>

<div class="sk-block" role="status" aria-label="加载中">
  <div class="artist-head sk-artist-head">
    <span class="sk sk-artist-photo"></span>
    <div class="artist-info sk-artist-info">
      <span class="sk sk-h1"></span>
      <span class="sk sk-subline"></span>
      <div class="artist-meta sk-meta">
        <span class="sk sk-chip"></span>
        <span class="sk sk-chip short"></span>
        <span class="sk sk-chip"></span>
      </div>
    </div>
  </div>

  <div class="sk-bio">
    <span class="sk sk-bio-line"></span>
    <span class="sk sk-bio-line short"></span>
  </div>

  <div class="tabs sk-tabs">
    <span class="sk sk-tab"></span>
    <span class="sk sk-tab"></span>
  </div>

  <TrackListSkeleton {rows} status={false} />
</div>
