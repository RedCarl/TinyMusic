<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { onDestroy } from 'svelte';
  import { api, type PlaylistRef } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account } from '../lib/account.svelte';
  import { go } from '../lib/router.svelte';

  // 平台列表：目前只有酷我，后续加平台在这里追加一项即可
  const PLATFORMS = [
    {
      id: 'kuwo',
      name: '酷我音乐',
      logo: 'https://h5s.kuwo.cn/www/kw-www/img/logo.ce08bf7.png'
    }
  ] as const;

  let webLogging = $state(false);
  let pollId: ReturnType<typeof setInterval> | null = null;
  let playlists = $state<PlaylistRef[]>([]);
  let plLoading = $state(false);
  let err = $state('');

  function stopPoll() {
    if (pollId) {
      clearInterval(pollId);
      pollId = null;
    }
    webLogging = false;
  }

  async function startLogin() {
    err = '';
    try {
      await api.loginOpen();
    } catch (e) {
      err = String(e);
      return;
    }
    webLogging = true;
    stopPoll();
    pollId = setInterval(async () => {
      try {
        const found = await api.loginProbe();
        if (found) {
          stopPoll();
          await api.loginClose().catch(() => {});
          account.uid = found;
          try {
            localStorage.setItem('kuwo_uid', found);
          } catch {
            /* ignore */
          }
          loadPlaylists();
        }
      } catch {
        /* 窗口已关，下一轮自然停 */
      }
    }, 1200);
  }

  function cancelLogin() {
    stopPoll();
    api.loginClose().catch(() => {});
  }

  function logout() {
    account.uid = '';
    playlists = [];
    try {
      localStorage.removeItem('kuwo_uid');
    } catch {
      /* ignore */
    }
  }

  async function loadPlaylists() {
    if (!account.uid) return;
    plLoading = true;
    err = '';
    try {
      playlists = await api.userPlaylists(account.provider, account.uid, 100);
    } catch (e) {
      err = String(e);
      playlists = [];
    } finally {
      plLoading = false;
    }
  }

  $effect(() => {
    if (account.uid) loadPlaylists();
  });

  onDestroy(() => {
    stopPoll();
    api.loginClose().catch(() => {});
  });
</script>

<div class="page">
  <h1>账号</h1>

  {#if err}<div class="err">{err}</div>{/if}

  {#each PLATFORMS as p (p.id)}
    <div class="acct-row">
      <span class="acct-logo">
        <img src={p.logo} alt={p.name} />
      </span>
      <div class="acct-main">
        <div class="acct-sub">
          {#if account.uid}
            已登录 · {account.uid}
          {:else}
            未登录
          {/if}
        </div>
      </div>
      {#if account.uid}
        <button class="mini" onclick={logout}>退出</button>
      {:else}
        <button class="mini primary" onclick={startLogin} disabled={webLogging}>
          {webLogging ? '等待登录…' : '登录'}
        </button>
      {/if}
    </div>
    {#if webLogging}
      <div class="acct-hint">
        在弹出的窗口里登录，完成后会自动关闭
        <button class="linkbtn" onclick={cancelLogin}>取消</button>
      </div>
    {/if}
  {/each}

  {#if account.uid}
    <div class="sub-h">我的歌单</div>
    {#if plLoading}
      <div class="muted small">读取中…</div>
    {:else if playlists.length === 0}
      <div class="muted small">这个账号还没有歌单</div>
    {:else}
      <div class="pllist">
        {#each playlists as pl (pl.id)}
          <button
            class="plrow"
            onclick={() =>
              go({ name: 'playlist', provider: pl.provider, playlistId: pl.id, title: pl.name })}
          >
            <span class="plname">{pl.name}</span>
            <span class="plnum">{pl.track_count} 首</span>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>
