<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import MorphIcon from './MorphIcon.svelte';
  import { fmtDuration, isBlocked, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { playTrack, player, toggle, isCurrent } from '../lib/player.svelte';
  import { requestDownload, isPending } from '../lib/download.svelte';
  import { openTrackMenu } from '../lib/trackMenu.svelte';
  import { appCfg } from '../lib/config.svelte';
  import { isFavorite, ensureFavorite, toggleFavorite } from '../lib/favorite.svelte';
  import { go } from '../lib/router.svelte';
  import { t, tc } from '../lib/i18n.svelte';

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

  const current = $derived(isCurrent(track));
  const loading = $derived(current && (player.status === 'loading' || player.loading));
  const playing = $derived(current && player.status === 'playing');
  const pending = $derived(isPending(track));
  const fav = $derived(isFavorite(track));

  async function toggleFav() {
    try {
      const next = await toggleFavorite(track);
      onFavoriteChange?.(track, next);
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
  const cloudOnly = $derived(track.provider === 's3');

  const playLabel = $derived(
    unavailable
      ? t('不可播放')
      : loading
        ? t('加载中')
        : playing
          ? t('暂停')
          : current
            ? t('继续播放')
            : t('播放')
  );

  // 挂载后查询一次收藏状态（避免依赖悬停事件）
  $effect(() => {
    if (appCfg.s3Enabled) void ensureFavorite(track);
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
      onFavorite: (f) => onFavoriteChange?.(track, f),
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
    {:else}
      <MorphIcon icon={playing ? icons.pause : icons.play} size={ICON_SIZE.inline} />
    {/if}
  </button>

  <div class="title">
    <span class="ttl" title={track.subtitle ? `${track.title} · ${track.subtitle}` : track.title}
      >{track.title}{#if track.subtitle}<span class="sub">{track.subtitle}</span>{/if}</span
    >
    {#if track.available_qualities?.includes('lossless')}
      <span class="badge sq" title={t('本曲有无损 FLAC')}>{t('无损')}</span>
    {/if}
    {#if track.availability === 'preview_only'}<span class="badge warn">{t('仅试听')}</span>{/if}
    {#if track.availability === 'unavailable'}<span class="badge mute">{t('不可用')}</span>{/if}
  </div>

  <div class="artist">
    <button
      class="link trunc"
      title={track.artist || t('未知艺人')}
      onclick={() => go({ name: 'artist', provider: track.provider, artistId: track.artist_id, title: track.artist })}
    >{track.artist || t('未知艺人')}</button>
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

  <div class="heat" title={track.hot ? t('热度 {n}', { n: track.hot }) : t('热度未知')}>
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
      title={cloudOnly ? t('网盘文件已在云端') : pending ? t('正在下载') : t('下载（含歌词与封面）')}
      aria-label={cloudOnly ? t('网盘文件') : pending ? t('正在下载') : tc('action', '下载')}
      disabled={unavailable || pending || cloudOnly}
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
        title={fav ? t('取消喜欢') : t('喜欢（收藏到网盘）')}
        aria-label={fav ? t('取消喜欢') : t('喜欢')}
        onclick={toggleFav}
      >
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.inline} />
      </button>
    {/if}
  </div>
</div>
