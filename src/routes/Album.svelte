<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, coverUrl, type AlbumPage } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { router } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import BatchBar from '../components/BatchBar.svelte';
  import { openTracksMenu } from '../lib/trackMenu.svelte';
  import BackLink from '../components/BackLink.svelte';
  import DetailPageSkeleton from '../components/DetailPageSkeleton.svelte';
  import { delayedVisible } from '../lib/loading.svelte';

  let route = $derived(router.route.name === 'album' ? router.route : null);
  let data = $state<AlbumPage | null>(null);
  let loading = $state(true);
  let err = $state('');

  const skel = delayedVisible(() => loading);

  $effect(() => {
    const r = route;
    if (!r) return;
    loading = true;
    err = '';
    data = null;
    api
      .album(r.provider, r.albumId, r.title, 0)
      .then((d) => (data = d))
      .catch((e) => (err = String(e)))
      .finally(() => (loading = false));
  });
</script>

<div class="page">
  <BackLink />
  {#if skel.visible}
    <DetailPageSkeleton rows={10} />
  {:else if loading}
    <!-- 等骨架屏延迟 -->
  {:else if data}
    <div class="head" role="group" oncontextmenu={(e) => openTracksMenu(e, data!.tracks)}>
      {#if data.cover}
        <img class="cover" src={coverUrl(data.cover, 500)} alt="" />
      {:else}
        <div class="cover ph" aria-hidden="true">
          <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
        </div>
      {/if}
      <div class="info">
        <h1>{data.name}</h1>
        <div class="muted">{data.artist} · {data.tracks.length} 首</div>
      </div>
    </div>

    <BatchBar tracks={data.tracks} label={`${data.tracks.length} 首`} />

    <div class="list">
      {#each data.tracks as t, i (t.provider + t.id)}
        <TrackRow track={t} index={i} queue={data.tracks} />
      {/each}
    </div>
  {:else if err}
    <div class="err">{err}</div>
  {/if}
</div>
