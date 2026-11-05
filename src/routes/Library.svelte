<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type PlaylistDetail, type Track } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account } from '../lib/account.svelte';
  import { go } from '../lib/router.svelte';
  import TrackRow from '../components/TrackRow.svelte';
  import BatchBar from '../components/BatchBar.svelte';

  let data = $state<PlaylistDetail | null>(null);
  let err = $state('');
  let loading = $state(false);

  $effect(() => {
    const uid = account.uid;
    if (!uid) {
      data = null;
      return;
    }
    loading = true;
    err = '';
    api
      .favorites(account.provider, uid)
      .then((d) => (data = d))
      .catch((e) => {
        err = String(e);
        data = null;
      })
      .finally(() => (loading = false));
  });

  const tracks = $derived<Track[]>(data?.tracks ?? []);
</script>

<div class="page">
  <div class="summary">
    喜欢{#if data} · {data.playlist.name} · {tracks.length} 首{/if}
  </div>

  {#if !account.uid}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
      </div>
      <p>登录后这里会显示账号的「喜欢」</p>
      <p class="muted small">收藏是账号数据，需要先登录</p>
      <button class="primary" onclick={() => go({ name: 'account' })}>
        <HugeiconsIcon icon={icons.login} size={ICON_SIZE.inline} />
        去登录
      </button>
    </div>
  {:else if loading}
    <div class="muted">读取中…</div>
  {:else if err}
    <div class="err">{err}</div>
    <div class="note">
      「喜欢」是账号里的 <code>type=MYFAVORITE</code> 歌单。如果提示没有该歌单，
      说明这个账号还没在客户端里点过红心。
    </div>
  {:else if tracks.length === 0}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.empty} />
      </div>
      <p>「喜欢」里还没有歌曲</p>
    </div>
  {:else}
    <BatchBar {tracks} />
    <div class="list">
      {#each tracks as t, i (t.provider + t.id + i)}
        <TrackRow track={t} index={i} queue={tracks} />
      {/each}
    </div>
  {/if}
</div>
