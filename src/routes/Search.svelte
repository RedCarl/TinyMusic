<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { api, type Track, type SearchPage, type AlbumRef } from '../lib/api';
  import { router, go, scrollMainTop } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import BatchBar from '../components/BatchBar.svelte';

  let kw = $state(router.route.name === 'search' ? (router.route.kw ?? '') : '');
  let page = $state(0);
  let loading = $state(false);
  let err = $state('');
  let result = $state<SearchPage | null>(null);
  let provider = $state('kuwo');
  let inputEl = $state<HTMLInputElement | null>(null);
  // 防止「搜索失败 → effect 重入 → 再失败」的死循环
  let autoTried = false;

  // 艺人区（搜索结果里第一条曲目的艺人）
  let artistName = $state('');
  let artistId = $state('');
  let albums = $state<AlbumRef[]>([]);
  let artistErr = $state('');

  onMount(() => {
    inputEl?.focus();
    if (kw.trim() && !autoTried) {
      autoTried = true;
      doSearch(0);
    }
  });

  async function loadArtist(items: Track[]) {
    artistName = '';
    artistId = '';
    albums = [];
    artistErr = '';
    const first = items.find((t) => t.artist_id);
    if (!first) return;
    artistName = first.artist;
    artistId = first.artist_id;
    try {
      // 用艺名再搜一次，按 ARTISTID 归组拿到专辑
      const ap = await api.artist(provider, first.artist_id, first.artist, 0);
      albums = ap.albums;
    } catch (e) {
      artistErr = String(e);
    }
  }

  async function doSearch(p = 0) {
    const q = kw.trim();
    if (!q) return;
    loading = true;
    err = '';
    page = p;
    try {
      result = await api.search(provider, q, p, 30);
      scrollMainTop();
      // 只在第一页取艺人专辑，翻页时不必重复请求
      if (p === 0) await loadArtist(result.items);
      else {
        artistName = '';
        albums = [];
      }
    } catch (e) {
      err = String(e);
      result = null;
    } finally {
      loading = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Enter') doSearch(0);
  }

  function openAlbum(a: AlbumRef) {
    go({ name: 'album', provider, albumId: a.id, title: a.name });
  }

  const totalPages = $derived(result ? Math.ceil(result.total / 30) : 0);
</script>

<div class="page">
  <div class="searchbar">
    <input
      bind:this={inputEl}
      bind:value={kw}
      onkeydown={onKey}
      placeholder="搜索歌曲、艺人、专辑…"
      spellcheck="false"
    />
    <button onclick={() => doSearch(0)} disabled={loading}>
      {loading ? '搜索中…' : '搜索'}
    </button>
  </div>

  {#if err}
    <div class="err">{err}</div>
  {/if}

  {#if result}
    {#if albums.length > 0}
      <div class="section-head">
        <span>{artistName} 的专辑</span>
        <button
          class="mini"
          onclick={() =>
            go({ name: 'artist', provider, artistId, title: artistName })}
        >
          查看艺人
        </button>
      </div>
      <div class="albumrow">
        {#each albums as a (a.id)}
          <button class="albumcard" onclick={() => openAlbum(a)}>
            {#if a.cover}
              <img src={a.cover} alt="" loading="lazy" />
            {:else}
              <div class="ph" aria-hidden="true">
                <HugeiconsIcon icon={icons.music} size={ICON_SIZE.empty} />
              </div>
            {/if}
            <div class="an">{a.name}</div>
          </button>
        {/each}
      </div>
    {/if}

    <div class="summary">
      共 {result.total.toLocaleString()} 条 · 第 {page + 1} / {totalPages} 页
    </div>

    <BatchBar tracks={result.items} />

    <div class="list">
      {#each result.items as t, i (t.provider + t.id)}
        <TrackRow track={t} index={i} queue={result.items} />
      {/each}
    </div>

    <div class="pager">
      <button disabled={page <= 0 || loading} onclick={() => doSearch(page - 1)}>上一页</button>
      <span>{page + 1}</span>
      <button disabled={page + 1 >= totalPages || loading} onclick={() => doSearch(page + 1)}>下一页</button>
    </div>
  {:else if !loading && !err}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.search} size={ICON_SIZE.empty} />
      </div>
      <p>输入关键词开始搜索</p>
      <div class="chips">
        {#each ['稻香', '夜曲', '青花瓷', '孤勇者', '起风了'] as c}
          <button onclick={() => { kw = c; doSearch(0); }}>{c}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
