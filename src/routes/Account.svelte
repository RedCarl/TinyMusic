<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { onDestroy, onMount } from 'svelte';
  import { api, type PlaylistRef } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account } from '../lib/account.svelte';
  import { go } from '../lib/router.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { copyText } from '../lib/toast.svelte';

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
  let playlists = $state<PlaylistRef[]>([]);
  let plLoading = $state(false);
  let err = $state('');

  // ---- 导入到网盘 ----
  let s3On = $state(false);
  let importPl = $state<PlaylistRef | null>(null);
  let importTarget = $state('');
  let importing = $state(false);
  let importProgress = $state('');
  let importMsg = $state('');
  let importErr = $state('');

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

    // 窗口是隐藏创建、等登录框就绪才显示，这里轮询到真正可见为止，
    // 期间按钮一直转圈，不会出现「点了没反应」。
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
          loadPlaylists();
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
    playlists = [];
    try {
      localStorage.removeItem('kuwo_uid');
    } catch {
      /* ignore */
    }
    // 清掉网页登录态，否则再登录会直接沿用上一个账号
    await api.loginPurge().catch(() => {});
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

  function openImport(pl: PlaylistRef) {
    importPl = pl;
    importTarget = pl.name;
    importProgress = '';
    importMsg = '';
    importErr = '';
    importing = false;
  }

  function closeImport() {
    if (importing) return;
    importPl = null;
  }

  async function runImport() {
    const pl = importPl;
    if (!pl || importing) return;
    importing = true;
    importProgress = '';
    importMsg = '';
    importErr = '';
    try {
      const res = await api.s3ImportPlaylist(
        account.provider,
        pl.id,
        pl.name,
        importTarget.trim(),
        (p) => {
          if (p.total && p.task_id.startsWith('batch-') && !p.task_id.startsWith('batch-done')) {
            importProgress = `第 ${p.downloaded + 1} / ${p.total} 首`;
          }
        }
      );
      importProgress = '';
      importMsg =
        `已导入 ${res.uploaded} 首到网盘歌单「${res.playlist}」` +
        (res.failed ? `，${res.failed} 首失败` : '');
      importing = false;
      setTimeout(() => {
        if (!importing) importPl = null;
      }, 2600);
    } catch (e) {
      importErr = String(e);
      importProgress = '';
    } finally {
      importing = false;
    }
  }

  $effect(() => {
    if (account.uid) loadPlaylists();
  });

  onMount(() => {
    api
      .settingsGet()
      .then((s) => (s3On = !!s.s3_enabled && !!s.s3_bucket))
      .catch(() => (s3On = false));
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
        <button class="mini primary" onclick={startLogin} disabled={loginBusy || webLogging}>
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
    {#if webLogging}
      <div class="acct-hint">
        在弹出的窗口里登录，完成后会自动关闭
        <button class="linkbtn" onclick={cancelLogin}>取消</button>
      </div>
    {/if}
  {/each}

  {#if account.uid}
    <div class="sub-h">我的歌单</div>
    {#if !s3On}
      <div class="acct-hint">
        登录只用于把平台歌单导入网盘。先到「网盘」页启用网盘配置。
      </div>
    {/if}
    {#if plLoading}
      <div class="muted small">读取中…</div>
    {:else if playlists.length === 0}
      <div class="muted small">这个账号还没有歌单</div>
    {:else}
      <div class="pllist">
        {#each playlists as pl (pl.id)}
          <div
            class="plrow"
            role="group"
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              openMenuAt(e.clientX, e.clientY, [
                {
                  label: '打开歌单',
                  icon: icons.playlist,
                  run: () =>
                    go({
                      name: 'playlist',
                      provider: pl.provider,
                      playlistId: pl.id,
                      title: pl.name,
                    }),
                },
                ...(s3On
                  ? [
                      {
                        label: '一键导入到网盘',
                        icon: icons.cloud,
                        run: () => openImport(pl),
                      },
                    ]
                  : []),
                { sep: true },
                {
                  label: '复制歌单 ID',
                  icon: icons.link,
                  run: () => copyText(pl.id, '已复制歌单 ID'),
                },
              ]);
            }}
          >
            <button
              class="plopen"
              onclick={() =>
                go({ name: 'playlist', provider: pl.provider, playlistId: pl.id, title: pl.name })}
            >
              <span class="plname">{pl.name}</span>
              <span class="plnum">{pl.track_count} 首</span>
            </button>
            {#if s3On}
              <button
                class="plicon"
                title="一键导入到网盘"
                aria-label="把歌单 {pl.name} 导入到网盘"
                onclick={() => openImport(pl)}
              >
                <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if importPl}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) closeImport();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label="导入歌单到网盘">
      <div class="modal-head">
        <div>
          <h2>导入到网盘</h2>
          <p class="hint">{importPl.name} · {importPl.track_count} 首 · 逐首下载后上传</p>
        </div>
        <button class="icon" onclick={closeImport} aria-label="关闭" disabled={importing}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      <div class="field">
        <div class="lbl">网盘歌单（目标目录）</div>
        <input bind:value={importTarget} placeholder="目标歌单名称" spellcheck="false" />
        <p class="hint" style="margin-top:8px">
          文件会存到 <code>前缀/歌单名/</code> 下，包含内嵌封面的音频、同名 .lrc 与封面图。
        </p>
      </div>

      {#if importProgress}
        <div class="modal-progress">
          <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
          {importProgress}
        </div>
      {/if}
      {#if importMsg}<div class="ok small">{importMsg}</div>{/if}
      {#if importErr}<div class="err small">{importErr}</div>{/if}

      <div class="modal-actions">
        <button class="mini" onclick={closeImport} disabled={importing}>关闭</button>
        <button
          class="primary btn-ico"
          onclick={runImport}
          disabled={importing || !importTarget.trim()}
        >
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
          {importing ? '导入中…' : '开始导入'}
        </button>
      </div>
    </div>
  </div>
{/if}
