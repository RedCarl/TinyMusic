<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { onDestroy } from 'svelte';
  import { api } from '../lib/api';
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
  let loginBusy = $state(false);
  let pollFails = 0;
  let pollId: ReturnType<typeof setInterval> | null = null;
  let err = $state('');

  function stopPoll() {
    if (pollId) {
      clearInterval(pollId);
      pollId = null;
    }
    webLogging = false;
  }

  async function startLogin() {
    if (loginBusy || webLogging) return;
    err = '';
    loginBusy = true;
    try {
      await api.loginOpen();
    } catch (e) {
      err = String(e);
      loginBusy = false;
      return;
    }

    const deadline = Date.now() + 20000;
    while (Date.now() < deadline) {
      if (!loginBusy) return; // 用户点了取消
      try {
        if (await api.loginVisible()) break;
      } catch {
        break;
      }
      await new Promise((r) => setTimeout(r, 250));
    }

    loginBusy = false;
    // 注意顺序：stopPoll() 会把 webLogging 置回 false，必须先停再置 true
    stopPoll();
    webLogging = true;
    pollFails = 0;
    pollId = setInterval(async () => {
      try {
        const found = await api.loginProbe();
        pollFails = 0;
        if (found) {
          stopPoll();
          await api.loginClose().catch(() => {});
          account.uid = found;
          try {
            localStorage.setItem('kuwo_uid', found);
          } catch {
            /* ignore */
          }
        }
      } catch {
        pollFails += 1;
        if (pollFails >= 3) {
          stopPoll();
          pollFails = 0;
        }
      }
    }, 1200);
  }

  function cancelLogin() {
    loginBusy = false;
    stopPoll();
    api.loginClose().catch(() => {});
  }

  async function logout() {
    loginBusy = false;
    stopPoll();
    await api.loginClose().catch(() => {});
    account.uid = '';
    try {
      localStorage.removeItem('kuwo_uid');
    } catch {
      /* ignore */
    }
    // 清掉网页登录态，否则再登录会直接沿用上一个账号
    await api.loginPurge().catch(() => {});
  }

  function openAccount() {
    if (account.uid) go({ name: 'accountplaylists' });
  }

  function onCardKey(e: KeyboardEvent) {
    if (!account.uid) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      openAccount();
    }
  }

  onDestroy(() => {
    stopPoll();
    api.loginClose().catch(() => {});
  });
</script>

<div class="page">
  <h1>账号</h1>

  {#if err}<div class="err">{err}</div>{/if}

  <div class="acct-grid">
    {#each PLATFORMS as p (p.id)}
      <div
        class="acct-row acct-card"
      class:clickable={!!account.uid}
      role="button"
      aria-disabled={!account.uid}
      tabindex={account.uid ? 0 : -1}
      title={account.uid ? '查看账号歌单' : undefined}
      onclick={openAccount}
      onkeydown={onCardKey}
    >
      <span class="acct-logo">
        <img src={p.logo} alt={p.name} />
      </span>
      <div class="acct-main">
        <div class="acct-name">{p.name}</div>
      </div>
        {#if account.uid}
          <button class="mini" onclick={(e) => { e.stopPropagation(); logout(); }}>登出</button>
        {:else}
          <button
            class="mini primary"
            onclick={(e) => { e.stopPropagation(); startLogin(); }}
            disabled={loginBusy || webLogging}
          >
            {#if loginBusy}
              <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
              正在打开…
            {:else if webLogging}
              等待登录…
            {:else}
              登录
            {/if}
          </button>
        {/if}
      </div>
    {/each}
  </div>
  {#if webLogging}
    <div class="acct-hint">
      在弹出的窗口里登录，完成后会自动关闭
      <button class="linkbtn" onclick={cancelLogin}>取消</button>
    </div>
  {/if}
</div>
