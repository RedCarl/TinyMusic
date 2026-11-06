<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, coverUrl, fmtCount, type PlaylistDetail } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { router } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import BatchBar from '../components/BatchBar.svelte';
  import { openTracksMenu } from '../lib/trackMenu.svelte';

  let route = $derived(router.route.name === 'playlist' ? router.route : null);
  let data = $state<PlaylistDetail | null>(null);
  let loading = $state(true);
  let err = $state('');

  $effect(() => {
    const r = route;
    if (!r) return;
    loading = true;
    err = '';
    data = null;
    api
      .playlist(r.provider, r.playlistId)
      .then((d) => {
        data = d;
      })
      .catch((e) => (err = String(e)))
      .finally(() => (loading = false));
  });

  async function toggleSave() {
    if (!data || saving) return;
    saving = true;
    try {
      if (isSaved) {
        await api.savedPlaylistRemove(data.playlist.provider, data.playlist.id);
        isSaved = false;
      } else {
        await api.savedPlaylistAdd(data.playlist);
        isSaved = true;
      }
    } catch (e) {
      err = String(e);
    } finally {
      saving = false;
    }
  }

  const totalDur = $derived(
    data ? data.tracks.reduce((s, t) => s + (t.duration_sec || 0), 0) : 0
  );
</script>

<div class="page">
  {#if loading}
    <div class="muted">加载歌单…</div>
  {:else if err}
    <div class="err">{err}</div>
    <div class="note">
      若提示不可见，可能是私密歌单或已被删除。酷我的账号歌单需要登录后才能列出，
      本客户端目前只支持公开歌单。
    </div>
  {:else if data}
    <div class="head" role="group" oncontextmenu={(e) => openTracksMenu(e, data!.tracks)}>
      {#if data.playlist.cover}
        <img class="cover" src={coverUrl(data.playlist.cover, 500)} alt="" />
      {:else}
        <div class="cover ph" aria-hidden="true">
          <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.empty} />
        </div>
      {/if}
      <div class="info">
        <h1>{data.playlist.name}</h1>
        <div class="line">
          {#if data.playlist.creator}<span>{data.playlist.creator}</span>{/if}
          <span class="dot">·</span>
          <span>{data.playlist.track_count} 首</span>
          {#if data.playlist.play_count}
            <span class="dot">·</span>
            <span>{fmtCount(data.playlist.play_count)} 播放</span>
          {/if}
          {#if totalDur}
            <span class="dot">·</span>
            <span>{Math.round(totalDur / 60)} 分钟</span>
          {/if}
        </div>
        {#if data.playlist.description}
          <div class="note desc">{data.playlist.description}</div>
        {/if}
      </div>
    </div>

    <BatchBar tracks={data.tracks} label={`${data.tracks.length} 首`} />

    {#if data.tracks.length === 0}
      <div class="empty"><p>这个歌单没有可显示的曲目</p></div>
    {:else}
      <div class="list">
        {#each data.tracks as t, i (t.provider + t.id + i)}
          <TrackRow track={t} index={i} queue={data.tracks} />
        {/each}
      </div>
    {/if}
  {/if}
</div>
