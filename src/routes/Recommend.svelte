<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, coverUrl, fmtCount, type PlaylistRef, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account } from '../lib/account.svelte';
  import { go } from '../lib/router.svelte';
  import { loginNetease } from '../lib/neteaseLogin';
  import { playTrack } from '../lib/player.svelte';
  import { showToast } from '../lib/toast.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import TrackListSkeleton from '../components/TrackListSkeleton.svelte';
  import { delayedVisible } from '../lib/loading.svelte';
  import ErrorNotice from '../components/ErrorNotice.svelte';

  let logged = $derived(!!account.netease.uid);

  let songs = $state<Track[]>([]);
  let recommended = $state<PlaylistRef[]>([]);
  let mine = $state<PlaylistRef[]>([]);
  let loading = $state(false);
  let err = $state('');
  let busy = $state(false);

  /** 悬浮播放按钮：记住正在准备的那一张，避免连点 */
  let fabBusy = $state('');

  /**
   * 右侧列表铺几首：按「左边卡片的高度 / 单行高度」算出来，
   * 这样两张卡永远等高，窗口变宽变窄都不用改死数字。
   */
  let cardEl = $state<HTMLElement | null>(null);
  let previewCount = $state(5);
  let sideH = $state(0);
  let preview = $derived(songs.slice(0, previewCount));

  function fitRows() {
    const card = cardEl;
    if (!card) return;
    const rows = card.parentElement?.querySelectorAll<HTMLElement>('.daily-side .row');
    const rowH = rows && rows.length > 0 ? rows[0].offsetHeight : 0;
    const h = card.offsetHeight;
    if (!rowH || !h) return;
    sideH = h;
    // 留一行余量：宁可少铺一首，也不让最后一行被裁掉
    const n = Math.max(3, Math.min(12, Math.floor(h / rowH)));
    if (n !== previewCount) previewCount = n;
  }

  const skel = delayedVisible(() => loading && songs.length === 0);

  // ---- 推荐歌单货架（横向） ----
  let shelfEl = $state<HTMLElement | null>(null);
  let canPrev = $state(false);
  let canNext = $state(false);

  function measureShelf() {
    const el = shelfEl;
    if (!el) return;
    canPrev = el.scrollLeft > 4;
    canNext = el.scrollLeft + el.clientWidth < el.scrollWidth - 4;
  }

  function pageShelf(dir: -1 | 1) {
    shelfEl?.scrollBy({
      left: dir * Math.max(240, (shelfEl.clientWidth ?? 0) - 80),
      behavior: 'smooth'
    });
  }

  async function load() {
    if (!logged) {
      songs = [];
      recommended = [];
      mine = [];
      return;
    }
    loading = true;
    err = '';
    // 三块彼此独立：任何一个失败都不该拖垮整页
    const [s, r, m] = await Promise.allSettled([
      api.recommendSongs('netease', 30),
      api.recommendPlaylists('netease'),
      api.myPlaylists('netease')
    ]);
    if (s.status === 'fulfilled') songs = s.value;
    else err = String(s.reason);
    if (r.status === 'fulfilled') recommended = r.value;
    if (m.status === 'fulfilled') mine = m.value;
    loading = false;
    requestAnimationFrame(measureShelf);
  }

  async function login() {
    if (busy) return;
    busy = true;
    err = '';
    try {
      await loginNetease();
      await load();
    } catch (e) {
      err = String(e);
    } finally {
      busy = false;
    }
  }

  /** 打开歌单详情（每日推荐在 core 里被当成 id=daily 的「假歌单」，走同一个页面） */
  function openPlaylist(provider: string, id: string, title: string) {
    go({ name: 'playlist', provider, playlistId: id, title });
  }

  function openDaily() {
    openPlaylist('netease', 'daily', '每日推荐');
  }

  /** 悬浮按钮：直接开始播放这张歌单（不进详情页） */
  async function playPlaylist(pl: { provider: string; id: string; name: string }) {
    const key = `${pl.provider}:${pl.id}`;
    if (fabBusy) return;
    // 每日推荐已经在内存里，不用再请求一次
    if (pl.id === 'daily' && songs.length > 0) {
      playTrack(songs[0], songs);
      return;
    }
    fabBusy = key;
    try {
      const d = await api.playlist(pl.provider, pl.id);
      if (d.tracks.length === 0) {
        showToast('这个歌单还没有可播放的曲目');
        return;
      }
      playTrack(d.tracks[0], d.tracks);
    } catch (e) {
      showToast(String(e));
    } finally {
      fabBusy = '';
    }
  }

  function stop(e: MouseEvent) {
    e.stopPropagation();
  }

  onMount(() => {
    void load();
    const onResize = () => measureShelf();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  });

  // 登录态变化后自动刷新（从账号页登录完回到这里）
  $effect(() => {
    if (logged && songs.length === 0 && !loading) void load();
  });

  // 卡片尺寸（含窗口缩放、字体变化）变了就重新算能铺几首
  $effect(() => {
    songs;
    const card = cardEl;
    if (!card) return;
    const ro = new ResizeObserver(() => requestAnimationFrame(fitRows));
    ro.observe(card);
    requestAnimationFrame(fitRows);
    return () => ro.disconnect();
  });

  // 歌单数据变了，货架的可翻页状态也要重算
  $effect(() => {
    recommended;
    requestAnimationFrame(measureShelf);
  });
</script>

<div class="page">
  <h1>推荐</h1>

  {#if !logged}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
      </div>
      <p>登录网易云后可以看到每日推荐与推荐歌单</p>
      <div class="chips">
        <button onclick={login} disabled={busy}>{busy ? '等待登录…' : '登录'}</button>
      </div>
    </div>
  {:else if skel.visible}
    <TrackListSkeleton rows={5} toolbar="none" />
  {:else}
    {#if err}<ErrorNotice text={err} />{/if}

    <!-- 1) 每日推荐：左边大卡片（点进详情），右边只铺前 5 首 -->
    {#if songs.length > 0}
      <div class="daily">
        <div
          class="daily-card"
          bind:this={cardEl}
          role="button"
          tabindex="0"
          aria-label="每日推荐，{songs.length} 首，查看全部"
          onclick={openDaily}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault();
              openDaily();
            }
          }}
        >
          <div class="daily-art">
            {#if songs[0]?.cover}
              <img src={coverUrl(songs[0].cover, 500)} alt="" />
            {:else}
              <span class="ph" aria-hidden="true">
                <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
              </span>
            {/if}
            <span class="daily-badge">每日推荐</span>
            <button
              class="fab"
              class:on={fabBusy === 'netease:daily'}
              title="播放每日推荐"
              aria-label="播放每日推荐"
              onclick={(e) => {
                stop(e);
                void playPlaylist({ provider: 'netease', id: 'daily', name: '每日推荐' });
              }}
            >
              <HugeiconsIcon icon={icons.play} size={ICON_SIZE.control} />
            </button>
          </div>
          <div class="daily-name">每日推荐</div>
          <div class="daily-sub">根据你的口味生成 · {songs.length} 首 · 每天 6:00 更新</div>
        </div>

        <div class="daily-side" style={sideH ? `height:${sideH}px` : undefined}>
          <div class="list">
            {#each preview as t, i (t.provider + t.id + i)}
              <TrackRow track={t} index={i} queue={songs} />
            {/each}
          </div>
        </div>
      </div>
    {:else if !loading}
      <div class="note">
        每日推荐为空。可能是账号当天还没有生成推荐，或登录态已过期（试试在账号页重新登录）。
      </div>
    {/if}

    <!-- 2) 推荐歌单：横向货架 -->
    {#if recommended.length > 0}
      <div class="section-head"><span>推荐歌单</span></div>
      <div class="shelf" class:can-prev={canPrev} class:can-next={canNext}>
        <div class="albumrow" bind:this={shelfEl} onscroll={measureShelf}>
          {#each recommended as pl (pl.id)}
            <div
              class="pl"
              role="button"
              tabindex="0"
              title={pl.name}
              aria-label={pl.name}
              onclick={() => openPlaylist(pl.provider, pl.id, pl.name)}
              onkeydown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault();
                  openPlaylist(pl.provider, pl.id, pl.name);
                }
              }}
            >
              <div class="art">
                {#if pl.cover}
                  <img src={coverUrl(pl.cover, 300)} alt="" loading="lazy" />
                {:else}
                  <span class="ph" aria-hidden="true">
                    <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.empty} />
                  </span>
                {/if}
                <button
                  class="fab"
                  class:on={fabBusy === `${pl.provider}:${pl.id}`}
                  title="播放 {pl.name}"
                  aria-label="播放 {pl.name}"
                  onclick={(e) => {
                    stop(e);
                    void playPlaylist(pl);
                  }}
                >
                  {#if fabBusy === `${pl.provider}:${pl.id}`}
                    <span class="spin"
                      ><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.control} /></span
                    >
                  {:else}
                    <HugeiconsIcon icon={icons.play} size={ICON_SIZE.control} />
                  {/if}
                </button>
              </div>
              <div class="nm">{pl.name}</div>
              <div class="sub">
                {#if pl.play_count}{fmtCount(pl.play_count)} 播放 · {/if}{pl.track_count} 首
              </div>
            </div>
          {/each}
        </div>
        {#if canPrev}
          <button
            class="shelf-nav prev"
            onclick={() => pageShelf(-1)}
            title="上一排"
            aria-label="上一排歌单"
          >
            <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
          </button>
        {/if}
        {#if canNext}
          <button
            class="shelf-nav next"
            onclick={() => pageShelf(1)}
            title="下一排"
            aria-label="下一排歌单"
          >
            <HugeiconsIcon icon={icons.forward} size={ICON_SIZE.inline} />
          </button>
        {/if}
      </div>
    {/if}

    <!-- 3) 我的歌单：同样的卡片，换行排列 -->
    {#if mine.length > 0}
      <div class="section-head"><span>我的歌单 · {mine.length}</span></div>
      <div class="wrap">
        {#each mine as pl (pl.id)}
          <div
            class="pl"
            role="button"
            tabindex="0"
            title={pl.name}
            aria-label={pl.name}
            onclick={() => openPlaylist(pl.provider, pl.id, pl.name)}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
                openPlaylist(pl.provider, pl.id, pl.name);
              }
            }}
          >
            <div class="art">
              {#if pl.cover}
                <img src={coverUrl(pl.cover, 300)} alt="" loading="lazy" />
              {:else}
                <span class="ph" aria-hidden="true">
                  <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.empty} />
                </span>
              {/if}
              <button
                class="fab"
                class:on={fabBusy === `${pl.provider}:${pl.id}`}
                title="播放 {pl.name}"
                aria-label="播放 {pl.name}"
                onclick={(e) => {
                  stop(e);
                  void playPlaylist(pl);
                }}
              >
                {#if fabBusy === `${pl.provider}:${pl.id}`}
                  <span class="spin"
                    ><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.control} /></span
                  >
                {:else}
                  <HugeiconsIcon icon={icons.play} size={ICON_SIZE.control} />
                {/if}
              </button>
            </div>
            <div class="nm">{pl.name}</div>
            <div class="sub">{pl.track_count} 首</div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  /* ---- 每日推荐：左卡片 + 右列表，两边等高 ---- */
  .daily {
    display: flex;
    gap: 18px;
    /* 卡片高度只由封面 + 文字决定，不被右侧拉伸（否则会互相追着长） */
    align-items: flex-start;
    padding: 4px 0 6px;
  }

  .daily-card {
    flex: none;
    display: flex;
    flex-direction: column;
    width: 244px;
    padding: 10px;
    border-radius: var(--r-lg);
    background-color: var(--bg2);
    text-align: left;
    cursor: pointer;
    box-shadow:
      inset 0 1px 0 var(--line-soft),
      inset 0 -1px 0 var(--line-soft);
    transition: background-color 160ms ease-out;
  }

  .daily-card:hover {
    background-color: color-mix(in oklch, var(--bg2), var(--fg) 5%);
  }

  .daily-art {
    position: relative;
    /* 4:5 竖版封面（同网易云那张「每日推荐」海报）：
       卡片更高一点，右侧就能多铺一两首，同时高度仍然是确定的 */
    flex: none;
    aspect-ratio: 4 / 5;
    border-radius: var(--r-md);
    overflow: hidden;
    background: linear-gradient(150deg, var(--cover-a), var(--cover-b));
  }

  .daily-art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  /* 悬浮时封面上压一层淡遮罩：既提示可点，又让播放键更清楚 */
  .daily-art::after,
  .pl .art::after {
    content: '';
    position: absolute;
    inset: 0;
    z-index: 0;
    background-color: rgb(0 0 0 / 0);
    transition: background-color 160ms ease-out;
    pointer-events: none;
  }

  .daily-card:hover .daily-art::after,
  .daily-card:focus-within .daily-art::after,
  .pl:hover .art::after,
  .pl:focus-within .art::after {
    background-color: rgb(0 0 0 / 0.22);
  }

  .daily-art .ph {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--fg3);
  }

  .daily-badge {
    position: absolute;
    left: 9px;
    top: 9px;
    padding: 2px 8px;
    border-radius: 999px;
    background-color: color-mix(in oklch, var(--ink) 62%, transparent);
    color: #fff;
    font-size: 10px;
    letter-spacing: 0.04em;
    backdrop-filter: blur(6px);
  }

  .daily-name {
    flex: none;
    margin-top: 11px;
    color: var(--fg);
    font-size: 14px;
    font-weight: 600;
  }

  .daily-sub {
    flex: none;
    margin-top: 3px;
    color: var(--fg3);
    font-size: 11px;
    line-height: 1.5;
  }

  .daily-side {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }

  /* 列表盒子撑满卡片高度；行数由 fitRows() 算好，余量摊到行间，不堆在底部 */
  .daily-side .list {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
  }

  .daily-side :global(.row) {
    grid-template-columns: 30px 30px minmax(0, 1fr) minmax(0, 104px) 52px 84px;
    padding: 7px 10px;
  }

  .daily-side :global(.row .album),
  .daily-side :global(.row .heat) {
    display: none;
  }

  /* ---- 歌单卡片（推荐歌单 / 我的歌单共用） ---- */
  .pl {
    flex: 0 0 auto;
    width: 148px;
    text-align: left;
    cursor: pointer;
    scroll-snap-align: start;
  }

  .pl .art {
    position: relative;
    width: 148px;
    height: 148px;
    border-radius: var(--r-md);
    overflow: hidden;
    background: linear-gradient(150deg, var(--cover-a), var(--cover-b));
  }

  .pl .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .pl .art .ph {
    width: 100%;
    height: 100%;
    display: grid;
    place-items: center;
    color: var(--fg3);
  }

  .pl .nm {
    display: -webkit-box;
    -webkit-line-clamp: 1;
    -webkit-box-orient: vertical;
    margin-top: 7px;
    color: var(--fg2);
    font-size: 12px;
    overflow: hidden;
    transition: color 140ms ease-out;
  }

  .pl:hover .nm {
    color: var(--fg);
  }

  .pl .sub {
    margin-top: 2px;
    color: var(--fg3);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* 悬浮播放键：压在封面右下角，和网易云一致 */
  .fab {
    position: absolute;
    right: 8px;
    bottom: 8px;
    z-index: 1;
    width: 34px;
    height: 34px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: #fff;
    background-color: color-mix(in oklch, var(--ink) 68%, transparent);
    box-shadow: 0 6px 16px rgb(0 0 0 / 0.35);
    backdrop-filter: blur(8px);
    opacity: 0;
    transform: translateY(4px);
    transition:
      opacity 150ms ease-out,
      transform 150ms ease-out,
      background-color 150ms ease-out;
  }

  .daily-card:hover .fab,
  .daily-card:focus-within .fab,
  .pl:hover .fab,
  .pl:focus-within .fab,
  .fab:focus-visible,
  .fab.on {
    opacity: 1;
    transform: none;
  }

  .fab:hover {
    background-color: color-mix(in oklch, var(--ink) 86%, transparent);
  }

  .fab:active {
    transform: scale(0.94);
  }

  /* 我的歌单：与货架同样的卡片尺寸，换行铺满 */
  .wrap {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    padding: 2px 2px 8px;
  }
</style>
