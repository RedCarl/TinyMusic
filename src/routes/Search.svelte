<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { api, coverUrl, type Track, type AlbumRef } from '../lib/api';
  import { router, go, scrollMainTop } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { openAlbumMenu } from '../lib/trackMenu.svelte';
  import TrackListSkeleton from '../components/TrackListSkeleton.svelte';
  import { delayedVisible } from '../lib/loading.svelte';
  import BatchBar from '../components/BatchBar.svelte';
  import {
    searchState,
    searchHistory,
    addHistory,
    removeHistory,
    clearHistory,
  } from '../lib/searchStore.svelte';

  let inputEl = $state<HTMLInputElement | null>(null);
  let mainEl: HTMLElement | null = null;
  let scrollTimer: number | undefined;

  const HOT = ['稻香', '夜曲', '青花瓷', '孤勇者', '起风了'];

  function routeKw(): string {
    return router.route.name === 'search' ? (router.route.kw ?? '') : '';
  }

  onMount(() => {
    mainEl = document.querySelector('main');
    mainEl?.addEventListener('scroll', onScroll, { passive: true });

    const kw = routeKw().trim();
    if (kw && (kw !== searchState.kw || !searchState.ran)) {
      searchState.kw = kw;
      doSearch(0);
    } else if (searchState.ran) {
      // 回到搜索页：恢复上次的滚动位置，不重新请求
      requestAnimationFrame(() => {
        mainEl?.scrollTo({ top: searchState.scrollTop, behavior: 'auto' });
      });
    } else {
      inputEl?.focus();
    }
  });

  onDestroy(() => {
    if (scrollTimer) clearTimeout(scrollTimer);
    // 离开页面时把最后一刻的滚动位置也记下来（scroll 事件有 120ms 防抖）
    if (mainEl) searchState.scrollTop = mainEl.scrollTop;
    mainEl?.removeEventListener('scroll', onScroll);
  });

  function onScroll() {
    if (!mainEl) return;
    if (scrollTimer) clearTimeout(scrollTimer);
    scrollTimer = setTimeout(() => {
      searchState.scrollTop = mainEl?.scrollTop ?? 0;
    }, 120) as unknown as number;
  }

  let albumsLoading = $state(false);

  async function loadArtist(items: Track[]) {
    searchState.artistName = '';
    searchState.artistId = '';
    searchState.albums = [];
    albumsLoading = false;
    const first = items.find((t) => t.artist_id);
    if (!first) return;
    searchState.artistName = first.artist;
    searchState.artistId = first.artist_id;
    // 专辑接口比搜索慢，先把骨架屏占住位置，避免歌曲列表出来后再“弹”出一排专辑
    albumsLoading = true;
    try {
      const ap = await api.artist('kuwo', first.artist_id, first.artist, 0);
      searchState.albums = ap.albums;
    } catch {
      searchState.albums = [];
    } finally {
      albumsLoading = false;
    }
  }

  async function doSearch(p = 0) {
    const q = searchState.kw.trim();
    if (!q) return;
    searchState.loading = true;
    searchState.err = '';
    searchState.page = p;
    searchState.ran = true;
    addHistory(q);
    try {
      const res = await api.search('kuwo', q, p, 30);
      searchState.result = res;
      searchState.scrollTop = 0;
      scrollMainTop();
      // 只在第一页取艺人专辑，翻页时不必重复请求
      if (p === 0) await loadArtist(res.items);
      else {
        searchState.artistName = '';
        searchState.artistId = '';
        searchState.albums = [];
        albumsLoading = false;
      }
    } catch (e) {
      searchState.err = String(e);
      searchState.result = null;
    } finally {
      searchState.loading = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') doSearch(0);
  }

  function pickHistory(q: string) {
    searchState.kw = q;
    doSearch(0);
  }

  function openAlbum(a: AlbumRef) {
    go({ name: 'album', provider: 'kuwo', albumId: a.id, title: a.name });
  }

  const totalPages = $derived(searchState.result ? Math.ceil(searchState.result.total / 30) : 0);
  const result = $derived(searchState.result);
  const loading = $derived(searchState.loading);
  /** 首次搜索才显示骨架；快请求不会闪一下 */
  const skel = delayedVisible(() => loading && !result);
  /** 艺人专辑是第二段请求，也用同样的延迟规则 */
  const albumsSkel = delayedVisible(() => albumsLoading);
</script>

<div class="page">
  <div class="searchbar">
    <input
      bind:this={inputEl}
      bind:value={searchState.kw}
      onkeydown={onKey}
      placeholder="搜索歌曲、艺人、专辑…"
      spellcheck="false"
    />
    <button onclick={() => doSearch(0)} disabled={loading}>
      {loading ? '搜索中…' : '搜索'}
    </button>
  </div>

  {#if searchHistory.items.length > 0}
    <div class="historyrow">
      <span class="hlabel">
        <HugeiconsIcon icon={icons.history} size={ICON_SIZE.inline} />
        最近搜索
      </span>
      <div class="hchips">
        {#each searchHistory.items as h (h.kw)}
          <span
            class="hchip"
            role="group"
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              openMenuAt(e.clientX, e.clientY, [
                { label: `搜索「${h.kw}」`, icon: icons.search, run: () => pickHistory(h.kw) },
                {
                  label: '删除这条记录',
                  icon: icons.trash,
                  danger: true,
                  run: () => removeHistory(h.kw),
                },
                { sep: true },
                {
                  label: '清空搜索历史',
                  icon: icons.trash,
                  danger: true,
                  run: () => clearHistory(),
                },
              ]);
            }}
          >
            <button class="hkw" onclick={() => pickHistory(h.kw)} title={h.kw}>{h.kw}</button>
            <button
              class="hx"
              onclick={() => removeHistory(h.kw)}
              title="删除这条记录"
              aria-label="删除搜索记录 {h.kw}"
            >
              <HugeiconsIcon icon={icons.close} size={12} />
            </button>
          </span>
        {/each}
      </div>
      <button class="hclear" onclick={clearHistory} title="清空搜索历史">清空</button>
    </div>
  {/if}

  {#if searchState.err}
    <div class="err">{searchState.err}</div>
  {/if}

  {#if skel.visible}
    <TrackListSkeleton rows={8} toolbar="search" />
  {:else if result}
    {#if albumsSkel.visible || (!albumsLoading && searchState.albums.length > 0)}
      <div class="section-head">
        <span>{searchState.artistName} 的专辑</span>
        <button
          class="mini"
          onclick={() =>
            go({
              name: 'artist',
              provider: 'kuwo',
              artistId: searchState.artistId,
              title: searchState.artistName,
            })}
        >
          查看艺人
        </button>
      </div>
      <div class="albumrow">
        {#if albumsSkel.visible}
          {#each Array(6) as _, i (i)}
            <div class="albumcard skc" aria-hidden="true">
              <span class="sk sk-album"></span>
              <span class="sk sk-line w1"></span>
            </div>
          {/each}
        {:else if !albumsLoading}
          {#each searchState.albums as a (a.id)}
          <button
            class="albumcard"
            onclick={() => openAlbum(a)}
            oncontextmenu={(e) => openAlbumMenu(e, a)}
          >
            {#if a.cover}
              <img src={coverUrl(a.cover, 300)} alt="" loading="lazy" />
            {:else}
              <div class="ph" aria-hidden="true">
                <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
              </div>
            {/if}
              <div class="an">{a.name}</div>
            </button>
          {/each}
        {/if}
      </div>
    {/if}

    <div class="summary">
      共 {result.total.toLocaleString()} 条 · 第 {searchState.page + 1} / {totalPages} 页
      {#if loading}<span class="hint">更新中…</span>{/if}
    </div>

    <BatchBar tracks={result.items} />

    <div class="list">
      {#each result.items as t, i (t.provider + t.id)}
        <TrackRow track={t} index={i} queue={result.items} />
      {/each}
    </div>

    <div class="pager">
      <button disabled={searchState.page <= 0 || loading} onclick={() => doSearch(searchState.page - 1)}
        >上一页</button
      >
      <span>{searchState.page + 1}</span>
      <button
        disabled={searchState.page + 1 >= totalPages || loading}
        onclick={() => doSearch(searchState.page + 1)}>下一页</button
      >
    </div>
  {:else if !loading}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.search} size={ICON_SIZE.empty} />
      </div>
      <p>{searchState.ran ? '没有找到匹配的歌曲' : '输入关键词开始搜索'}</p>
      <div class="chips">
        {#each HOT as c}
          <button onclick={() => pickHistory(c)}>{c}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
