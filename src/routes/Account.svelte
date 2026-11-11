<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account, setUid, clearNetease } from '../lib/account.svelte';
  import { go, router } from '../lib/router.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { loginNetease, syncNeteaseSession } from '../lib/neteaseLogin';

  // 平台列表：所有平台都是一个「登录」按钮，点了各自打开网页登录窗口
  const PLATFORMS = [
    {
      id: 'kuwo',
      name: '酷我音乐',
      logo: 'https://h5s.kuwo.cn/www/kw-www/img/logo.ce08bf7.png'
    }
  ] as const;

  // ---------- 酷我 ----------
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
          setUid(found);
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

  async function logout() {
    loginBusy = false;
    stopPoll();
    await api.loginClose().catch(() => {});
    setUid('');
    // 清掉网页登录态，否则再登录会直接沿用上一个账号
    await api.loginPurge().catch(() => {});
  }

  // ---------- 网易云 ----------
  let neBusy = $state(false);
  let neErr = $state('');

  async function neteaseLogin() {
    if (neBusy) return;
    neBusy = true;
    neErr = '';
    try {
      const s = await loginNetease();
      showToast(`已登录网易云：${s.nickname || s.user_id}`);
    } catch (e) {
      neErr = String(e);
    } finally {
      neBusy = false;
    }
  }

  async function neteaseLogout() {
    await api.logout('netease').catch(() => {});
    clearNetease();
    // 登出后「推荐」会从侧边栏消失，停在那一页就成了无入口的死页面
    if (router.route.name === 'recommend') go({ name: 'search' });
    showToast('已登出网易云');
  }

  function openNeteasePlaylists() {
    if (account.netease.uid) go({ name: 'recommend' });
  }

  function onCardKey(e: KeyboardEvent, has: boolean, run: () => void) {
    if (!has) return;
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      run();
    }
  }

  onMount(() => {
    // 以后端会话为准，纠正 localStorage 里的展示缓存
    void syncNeteaseSession();
  });

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
      <!-- 未登录时卡片不是按钮：否则 aria-disabled 会把里面的「登录」按钮
           一起标成不可用，读屏/自动化都点不到 -->
      <div
        class="acct-row acct-card"
        class:clickable={!!account.uid}
        role={account.uid ? 'button' : undefined}
        tabindex={account.uid ? 0 : undefined}
        aria-label={p.name}
        title={account.uid ? '查看账号歌单' : undefined}
        onclick={() => account.uid && go({ name: 'accountplaylists' })}
        onkeydown={(e) =>
          onCardKey(e, !!account.uid, () => go({ name: 'accountplaylists' }))}
      >
        <span class="acct-logo">
          <img src={p.logo} alt={p.name} />
        </span>
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

    <!-- 网易云：目录 + 推荐 -->
    <div
      class="acct-row acct-card"
      class:clickable={!!account.netease.uid}
      role={account.netease.uid ? 'button' : undefined}
      tabindex={account.netease.uid ? 0 : undefined}
      aria-label="网易云音乐"
      title={account.netease.uid ? '查看每日推荐' : undefined}
      onclick={openNeteasePlaylists}
      onkeydown={(e) => onCardKey(e, !!account.netease.uid, openNeteasePlaylists)}
    >
      <span class="acct-logo wordmark">
        <img src="/netease-logo.png" alt="网易云音乐" />
      </span>
      {#if account.netease.uid}
        <span class="acct-name">{account.netease.nickname || account.netease.uid}</span>
        <button class="mini" onclick={(e) => { e.stopPropagation(); neteaseLogout(); }}>登出</button>
      {:else}
        <button
          class="mini primary"
          onclick={(e) => { e.stopPropagation(); neteaseLogin(); }}
          disabled={neBusy}
          title="在应用内打开网易云网页完成登录"
        >
          {#if neBusy}
            <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
            等待登录…
          {:else}
            登录
          {/if}
        </button>
      {/if}
    </div>
  </div>

  {#if neErr}
    <div class="err">{neErr}</div>
  {/if}
</div>

<style>
  /* 网易云用的是「红圆标 + 白色字标」，白底会把文字吃掉，
     所以这一格只去掉底板和描边，高度仍保持 38px 以便和酷我卡片对齐。 */
  .acct-logo.wordmark {
    background: none;
    box-shadow: none;
    border-radius: 0;
  }
</style>
