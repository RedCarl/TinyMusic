<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, coverUrl, type ArtistPage } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { router, go } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import { openAlbumMenu } from '../lib/trackMenu.svelte';
  import BackLink from '../components/BackLink.svelte';
  import SkeletonList from '../components/SkeletonList.svelte';

  let route = $derived(router.route.name === 'artist' ? router.route : null);
  let data = $state<ArtistPage | null>(null);
  let loading = $state(true);
  let err = $state('');
  let tab = $state<'songs' | 'albums'>('songs');
  let bioOpen = $state(false);
  let picErr = $state(false);

  $effect(() => {
    const r = route;
    if (!r) return;
    loading = true;
    err = '';
    data = null;
    picErr = false;
    bioOpen = false;
    tab = 'songs';
    api
      .artist(r.provider, r.artistId, r.title, 0)
      .then((d) => (data = d))
      .catch((e) => (err = String(e)))
      .finally(() => (loading = false));
  });

  const bio = $derived(data?.desc ?? '');
  const bioTooLong = $derived(bio.length > 180);
</script>

<div class="page">
  <BackLink />
  {#if data}
    <div class="artist-head">
      {#if data.pic && !picErr}
        <img
          class="artist-photo"
          src={coverUrl(data.pic, 500)}
          alt={data.name}
          onerror={() => (picErr = true)}
        />
      {:else}
        <div class="artist-photo avatar">{(data.name || '?').slice(0, 1)}</div>
      {/if}

      <div class="artist-info">
        <h1>{data.name}</h1>
        <div class="muted">{data.tracks.length} 首曲目 · {data.albums.length} 张专辑</div>

        {#if data.meta.length}
          <div class="artist-meta">
            {#each data.meta as [k, v] (k)}
              <span class="meta-chip"><em>{k}</em>{v}</span>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    {#if bio}
      <div class="artist-bio" class:clamp={bioTooLong && !bioOpen}>
        {bio}
      </div>
      {#if bioTooLong}
        <button class="bio-toggle" onclick={() => (bioOpen = !bioOpen)}>
          {bioOpen ? '收起' : '展开简介'}
        </button>
      {/if}
    {/if}

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
          <button
            class="card"
            onclick={() => go({ name: 'album', provider: 'kuwo', albumId: a.id, title: a.name })}
            oncontextmenu={(e) => openAlbumMenu(e, a)}
          >
            {#if a.cover}
              <img src={coverUrl(a.cover, 300)} alt="" loading="lazy" />
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
    <SkeletonList rows={8} />
  {:else if err}
    <div class="err">{err}</div>
  {/if}
</div>
