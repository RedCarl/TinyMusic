<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, fmtDuration, isBlocked, type Track, type Quality } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { playTrack, player } from '../lib/player.svelte';
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

  let dl = $state({ running: false, pct: 0, msg: '' });
  let fav = $state(false);
  let favLoaded = $state(false);

  const isCurrent = $derived(
    player.current?.id === track.id && player.current?.provider === track.provider
  );

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
      } else {
        await api.favoriteAdd(track);
        fav = true;
        onFavoriteChange?.(track, true);
      }
    } catch (e) {
      dl.msg = String(e);
    }
  }

  async function download() {
    if (dl.running) return;
    dl = { running: true, pct: 0, msg: '' };
    try {
      const q = (localStorage.getItem('quality') as Quality) || 'high';
      await api.downloadStart(track, q, (p) => {
        if (p.total) dl.pct = Math.min(100, Math.floor((p.downloaded / p.total) * 100));
        else dl.pct = 0;
      });
      // 后台任务已启动，进度由列表页统一展示；这里给个提示
      dl = { running: false, pct: 100, msg: '已加入下载' };
      setTimeout(() => (dl = { running: false, pct: 0, msg: '' }), 2500);
    } catch (e) {
      const msg = String(e);
      dl = {
        running: false,
        pct: 0,
        msg: msg.includes('unavailable') || msg.includes('会员') ? '不可下载' : '下载失败'
      };
      setTimeout(() => (dl = { running: false, pct: 0, msg: '' }), 3000);
    }
  }

  const unavailable = $derived(isBlocked(track.availability));
  const vipOnly = $derived(track.availability === 'vip_only');

  // 挂载后查询一次收藏状态（避免依赖悬停事件）
  $effect(() => {
    checkFav();
  });
</script>

<div
  class="row"
  class:current={isCurrent}
  class:unavailable
>
  <div class="idx">
    {#if isCurrent && player.playing}
      <span class="bars"><i></i><i></i><i></i></span>
    {:else}
      {index + 1}
    {/if}
  </div>

  <button
    class="play"
    title={vipOnly ? '会员曲目（可播放）' : unavailable ? '已下架' : '播放'}
    aria-label={unavailable ? '不可播放' : '播放'}
    disabled={unavailable}
    onclick={() => playTrack(track, queue)}
  >
    <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
  </button>

  <div class="title">
    <div class="t1">
      {track.title}
      {#if track.subtitle}<span class="sub">{track.subtitle}</span>{/if}
      {#if track.availability === 'preview_only'}<span class="badge warn">仅试听</span>{/if}
      {#if vipOnly}<span class="badge warn">会员曲</span>{/if}
      {#if track.availability === 'unavailable'}<span class="badge mute">不可用</span>{/if}
    </div>
    <div class="t2">
      <button
        class="link"
        onclick={() => go({ name: 'artist', provider: track.provider, artistId: track.artist_id, title: track.artist })}
      >{track.artist || '未知艺人'}</button>
      {#if track.album}
        <span class="dot">·</span>
        <button
          class="link"
          onclick={() => go({ name: 'album', provider: track.provider, albumId: track.album_id, title: track.album })}
        >{track.album}</button>
      {/if}
    </div>
  </div>

  <div class="dur">{fmtDuration(track.duration_sec)}</div>

  <div class="ops">
    {#if dl.msg}
      <span class="tip">{dl.msg}</span>
    {:else if dl.running}
      <span class="tip">{dl.pct}%</span>
    {/if}
    <button
      class="icon"
      title="下载"
      aria-label="下载"
      disabled={unavailable || dl.running}
      onclick={download}
    >
      <HugeiconsIcon icon={icons.download} size={ICON_SIZE.inline} />
    </button>
    <button
      class="icon"
      class:on={fav}
      title={fav ? '取消收藏' : '收藏'}
      aria-label={fav ? '取消收藏' : '收藏'}
      onclick={toggleFav}
    >
      <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.inline} />
    </button>
  </div>
</div>
