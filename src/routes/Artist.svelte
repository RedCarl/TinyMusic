<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type ArtistPage } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { router } from '../lib/router.svelte';
  import { go } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';

  let route = $derived(router.route.name === 'artist' ? router.route : null);
  let data = $state<ArtistPage | null>(null);
  let loading = $state(true);
  let err = $state('');
  let tab = $state<'songs' | 'albums'>('songs');

  $effect(() => {
    const r = route;
    if (!r) return;
    loading = true;
    err = '';
    data = null;
    api
      .artist(r.provider, r.artistId, r.title, 0)
      .then((d) => (data = d))
      .catch((e) => (err = String(e)))
      .finally(() => (loading = false));
  });
</script>

<div class="page">
  {#if data}
    <div class="artist-head">
      <div class="avatar">{(data.name || '?').slice(0, 1)}</div>
      <div>
        <h1>{data.name}</h1>
        <div class="muted">{data.tracks.length} 首曲目 · {data.albums.length} 张专辑</div>
      </div>
    </div>

    <div class="tabs">
      <button class:on={tab === 'songs'} onclick={() => (tab = 'songs')}>歌曲</button>
      <button class:on={tab === 'albums'} onclick={() => (tab = 'albums')}>专辑</button>
    </div>

    {#if tab === 'songs'}
      <div class="list">
        {#each data.tracks as t, i (t.provider + t.id)}
          <TrackRow track={t} index={i} queue={data.tracks} />
        {/each}
      </div>
    {:else}
      <div class="grid">
        {#each data.albums as a (a.id)}
          <button class="card" onclick={() => go({ name: 'album', provider: 'kuwo', albumId: a.id, title: a.name })}>
            {#if a.cover}
              <img src={a.cover} alt="" />
            {:else}
              <div class="ph" aria-hidden="true">
                <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
              </div>
            {/if}
            <div class="cn">{a.name}</div>
          </button>
        {/each}
      </div>
    {/if}
  {:else if loading}
    <div class="muted">加载中…</div>
  {:else if err}
    <div class="err">{err}</div>
  {/if}
</div>
