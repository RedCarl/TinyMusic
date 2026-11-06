<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, fmtDuration, isBlocked, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { playTrack, player, toggle, isCurrent } from '../lib/player.svelte';
  import { requestDownload, isPending } from '../lib/download.svelte';
  import { openTrackMenu } from '../lib/trackMenu.svelte';
  import { appCfg } from '../lib/config.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { go } from '../lib/router.svelte';

  let {
    track,
    index = 0,
    queue = [],
    onFavoriteChange
  }: {
    track: Track;
    index?: number;
    queue?: Track[];
    onFavoriteChange?: (t: Track, fav: boolean) => void;
  } = $props();

  let msg = $state('');
  let fav = $state(false);
  let favLoaded = $state(false);

  const current = $derived(isCurrent(track));
  const loading = $derived(current && (player.status === 'loading' || player.loading));
  const playing = $derived(current && player.status === 'playing');
  const pending = $derived(isPending(track));

  async function checkFav() {
    if (favLoaded) return;
    favLoaded = true;
    try {
      fav = await api.favoriteCheck(track.provider, track.id);
    } catch {
      /* 忽略 */
    }
  }

  async function toggleFav() {
    try {
      if (fav) {
        await api.favoriteRemove(track.provider, track.id);
        fav = false;
        onFavoriteChange?.(track, false);
        showToast('已取消喜欢');
      } else {
        await api.favoriteAdd(track);
        fav = true;
        onFavoriteChange?.(track, true);
        showToast('已收藏到网盘');
      }
    } catch (e) {
      msg = String(e);
    }
  }

  function onPlay() {
    if (current) {
      toggle();
    } else {
      void playTrack(track, queue);
    }
  }

  const unavailable = $derived(isBlocked(track.availability));
  const vipOnly = $derived(track.availability === 'vip_only');

  const playLabel = $derived(
    unavailable
      ? '不可播放'
      : loading
        ? '加载中'
        : playing
          ? '暂停'
          : current
            ? '继续播放'
            : vipOnly
              ? '会员曲目（可播放）'
              : '播放'
  );

  // 挂载后查询一次收藏状态（避免依赖悬停事件）
  $effect(() => {
    checkFav();
  });
</script>

<div
  class="row"
  class:current
  class:unavailable
  role="group"
  oncontextmenu={(e) =>
    openTrackMenu(e, track, {
      queue,
      onFavorite: (f) => {
        fav = f;
        onFavoriteChange?.(track, f);
      },
    })}
>
  <div class="idx">
    {#if current && playing}
      <span class="bars"><i></i><i></i><i></i></span>
    {:else}
      {index + 1}
    {/if}
  </div>

  <button
    class="play"
    title={playLabel}
    aria-label={playLabel}
    disabled={unavailable}
    onclick={onPlay}
  >
    {#if loading}
      <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
    {:else if playing}
      <HugeiconsIcon icon={icons.pause} size={ICON_SIZE.inline} />
    {:else}
      <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
    {/if}
  </button>

  <div class="title">
    <span class="ttl" title={track.subtitle ? `${track.title} · ${track.subtitle}` : track.title}
      >{track.title}{#if track.subtitle}<span class="sub">{track.subtitle}</span>{/if}</span
    >
    {#if track.availability === 'preview_only'}<span class="badge warn">仅试听</span>{/if}
    {#if vipOnly}<span class="badge warn">会员曲</span>{/if}
    {#if track.availability === 'unavailable'}<span class="badge mute">不可用</span>{/if}
  </div>

  <div class="artist">
    <button
      class="link trunc"
      title={track.artist || '未知艺人'}
      onclick={() => go({ name: 'artist', provider: track.provider, artistId: track.artist_id, title: track.artist })}
    >{track.artist || '未知艺人'}</button>
  </div>

  <div class="album">
    {#if track.album}
      <button
        class="link trunc"
        title={track.album}
        onclick={() => go({ name: 'album', provider: track.provider, albumId: track.album_id, title: track.album })}
      >{track.album}</button>
    {/if}
  </div>

  <div class="heat" title={track.hot ? `热度 ${track.hot}` : '热度未知'}>
    {#if (track.hot ?? 0) > 0}
      <span class="heat-bar"><i style="width:{Math.min(100, track.hot ?? 0)}%"></i></span>
    {/if}
  </div>

  <div class="dur">{fmtDuration(track.duration_sec)}</div>

  <div class="ops">
    {#if msg}
      <span class="tip">{msg}</span>
    {/if}
    <button
      class="icon"
      title={pending ? '正在下载' : '下载（含歌词与封面）'}
      aria-label={pending ? '正在下载' : '下载'}
      disabled={unavailable || pending}
      onclick={() => requestDownload([track])}
    >
      {#if pending}
        <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
      {:else}
        <HugeiconsIcon icon={icons.download} size={ICON_SIZE.inline} />
      {/if}
    </button>
    {#if appCfg.s3Enabled}
      <button
        class="icon"
        class:on={fav}
        title={fav ? '取消喜欢' : '喜欢（收藏到网盘）'}
        aria-label={fav ? '取消喜欢' : '喜欢'}
        onclick={toggleFav}
      >
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.inline} />
      </button>
    {/if}
  </div>
</div>
