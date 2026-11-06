<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { appCfg, refreshAppCfg } from '../lib/config.svelte';
  import { go } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import BatchBar from '../components/BatchBar.svelte';
  import SkeletonList from '../components/SkeletonList.svelte';

  let tracks = $state<Track[]>([]);
  let loading = $state(true);
  let syncing = $state(false);
  let err = $state('');

  async function load(pull: boolean) {
    loading = true;
    err = '';
    try {
      tracks = pull ? await api.favoritesPull() : await api.favoriteList();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    await refreshAppCfg();
    await load(appCfg.s3Enabled);
  });

  async function sync() {
    syncing = true;
    err = '';
    try {
      tracks = await api.favoritesPull();
    } catch (e) {
      err = String(e);
    } finally {
      syncing = false;
    }
  }

  function onFavChange(t: Track, fav: boolean) {
    if (!fav) {
      tracks = tracks.filter((x) => !(x.id === t.id && x.provider === t.provider));
    }
  }
</script>

<div class="page">
  <div class="summary">
    喜欢
    <span class="muted">· {tracks.length} 首</span>
    <button class="mini btn-ico" onclick={sync} disabled={syncing || !appCfg.s3Enabled}>
      <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
      {syncing ? '同步中…' : '从网盘同步'}
    </button>
  </div>

  {#if err}<div class="err">{err}</div>{/if}

  {#if !appCfg.loaded || loading}
    <SkeletonList rows={6} />
  {:else if !appCfg.s3Enabled}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
      </div>
      <p>喜欢存在网盘里</p>
      <button class="primary btn-ico" onclick={() => go({ name: 'settings' })}>启用网盘</button>
    </div>
  {:else if tracks.length === 0}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
      </div>
      <p>还没有喜欢的歌曲</p>
      <p class="muted small">点歌曲右边的红心，会自动收藏并下载到网盘</p>
    </div>
  {:else}
    <BatchBar {tracks} />
    <div class="list">
      {#each tracks as t, i (t.provider + t.id)}
        <TrackRow track={t} index={i} queue={tracks} onFavoriteChange={onFavChange} />
      {/each}
    </div>
  {/if}
</div>
