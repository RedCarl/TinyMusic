<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { listen } from '@tauri-apps/api/event';
  import { icons, ICON_SIZE } from './lib/icons';
  import { router, go } from './lib/router.svelte';
  import { api } from './lib/api';
  import { updateState, checkUpdatesSilently } from './lib/update.svelte';
  import {
    player,
    bindAudio,
    restoreVolume,
    onEnded,
    toggle,
    loadQualityCapabilities,
    setPreferredQuality,
    refreshTrackQualities,
    fallbackFromLossless,
    next,
    prev,
    syncPlayback,
    persistPlayback,
    consumePendingSeek,
    clearPendingSeek,
    recoverSource,
  } from './lib/player.svelte';
  import PlayerBar from './components/PlayerBar.svelte';
  import TitleBar from './components/TitleBar.svelte';
  import WindowControls from './components/WindowControls.svelte';
  import DownloadDialog from './components/DownloadDialog.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import Toast from './components/Toast.svelte';
  import Search from './routes/Search.svelte';
  import Recommend from './routes/Recommend.svelte';
  import Playlist from './routes/Playlist.svelte';
  import Account from './routes/Account.svelte';
  import AccountPlaylists from './routes/AccountPlaylists.svelte';
  import Track from './routes/Track.svelte';
  import Artist from './routes/Artist.svelte';
  import Album from './routes/Album.svelte';
  import Downloads from './routes/Downloads.svelte';
  import Library from './routes/Library.svelte';
  import Settings from './routes/Settings.svelte';
  import Cloud from './routes/Cloud.svelte';
  import Lyrics from './routes/Lyrics.svelte';
  import { appCfg, refreshAppCfg } from './lib/config.svelte';
  import { account, hasNetease } from './lib/account.svelte';
  import { syncNeteaseSession } from './lib/neteaseLogin';
  import { showToast } from './lib/toast.svelte';

  // 自绘标题栏只出现在 Windows + Tauri（见 main.ts 的 data-chrome）
  const customChrome = document.documentElement.dataset.chrome === 'custom';

  let audio = $state<HTMLAudioElement | null>(null);
  let mainEl = $state<HTMLElement | null>(null);

  // 侧边栏选中背景：位置跟着当前菜单滑动
  let asideEl = $state<HTMLElement | null>(null);
  let indTop = $state(0);
  let indH = $state(0);
  let indShow = $state(false);
  let indReady = $state(false);

  function measureNav() {
    const aside = asideEl;
    if (!aside) return;
    const btn = aside.querySelector<HTMLElement>('button.on');
    if (!btn) {
      indShow = false;
      return;
    }
    const ar = aside.getBoundingClientRect();
    const br = btn.getBoundingClientRect();
    indTop = br.top - ar.top;
    indH = br.height;
    indShow = true;
  }

  $effect(() => {
    // 依赖当前菜单和「喜欢」是否出现（导航项数量会变）
    activeNav;
    appCfg.s3Enabled;
    // 登录/登出会增删「推荐」这一项，导航指示条要跟着重量
    navTop;
    const raf = requestAnimationFrame(() => {
      measureNav();
      if (!indReady) requestAnimationFrame(() => (indReady = true));
    });
    return () => cancelAnimationFrame(raf);
  });

  // 路由变化时把主区域滚回顶部。
  // 搜索页例外：它自己保存/恢复滚动位置，不能在这里重置。
  $effect(() => {
    const name = router.route.name;
    if (name === 'search') return;
    mainEl?.scrollTo({ top: 0 });
  });

  let bootErr = $state('');

  /** 播放失败：FLAC 不被 WebView 支持时自动退回 320k，其余错误照实显示 */
  async function handlePlayError(err: unknown) {
    const name = (err as DOMException)?.name;
    if (name === 'NotSupportedError' && (await fallbackFromLossless())) {
      showToast('当前系统不支持 FLAC，已切到 320k');
      player.error = '';
      return;
    }
    // 直链过期也会以 NotSupportedError / 空源的形式冒出来，先换一条新直链试一次
    if (await recoverSource()) {
      player.error = '';
      return;
    }
    player.error = '播放失败：' + String(err);
    player.status = 'error';
    player.playing = false;
  }

  onMount(() => {
    restoreVolume();
    void loadQualityCapabilities();
    // 音质以后端设置为准；旧 localStorage 只做一次性迁移（high 实际是 128k）
    import('./lib/api').then(async ({ api: a }) => {
      try {
        const s = await a.settingsGet();
        const legacy = localStorage.getItem('quality');
        if (legacy) {
          const mapped = legacy === 'high' ? 'standard' : legacy;
          try {
            await a.qualitySet(mapped as never);
            setPreferredQuality(mapped);
          } catch {
            setPreferredQuality(s.default_quality);
          }
          localStorage.removeItem('quality');
        } else {
          setPreferredQuality(s.default_quality);
        }
        if (s.s3_enabled && s.s3_bucket) a.favoritesPull().catch(() => {});
      } catch {
        /* 后端不可用时保持默认 */
      }
    });
    if (player.current) void refreshTrackQualities(player.current);
    refreshAppCfg().then((s) => {
      if (s?.s3_enabled && s.s3_bucket) api.favoritesPull().catch(() => {});
    });
    bindAudio((action, value) => {
      const el = audio;
      if (!el) return;
      switch (action) {
        case 'load':
          el.src = player.src;
          el.load();
          break;
        case 'play':
          el.play().catch((e) => void handlePlayError(e));
          break;
        case 'pause':
          el.pause();
          break;
        case 'seek':
          // 恢复的上次记录还没取流时拖动进度：只更新 UI，等 play 时再落到 audio
          if (!el.src) break;
          try {
            el.currentTime = value ?? 0;
          } catch {
            /* metadata 还没就绪时忽略 */
          }
          break;
      }
    });

    // 托盘菜单 → 前端动作
    let unlisten: (() => void) | undefined;
    listen<string>('tray://action', (e) => {
      switch (e.payload) {
        case 'toggle':
          toggle();
          break;
        case 'next':
          next();
          break;
        case 'prev':
          prev();
          break;
      }
    })
      .then((fn) => (unlisten = fn))
      .catch(() => {
        /* 非 Tauri 环境（纯浏览器调试）忽略 */
      });

    const saveRecord = () => {
      if (audio) player.progress = audio.currentTime;
      persistPlayback(true);
    };
    const onVisibility = () => {
      if (document.hidden) saveRecord();
    };
    window.addEventListener('pagehide', saveRecord);
    window.addEventListener('beforeunload', saveRecord);
    document.addEventListener('visibilitychange', onVisibility);

    // 静默更新检查：启动后隔一会儿查一次（别和首屏抢网络），之后每 6 小时再看一眼
    const firstCheck = setTimeout(() => void checkUpdatesSilently(), 2500);
    const updateTimer = setInterval(() => void checkUpdatesSilently(), 6 * 60 * 60 * 1000);

    return () => {
      unlisten?.();
      clearTimeout(firstCheck);
      clearInterval(updateTimer);
      window.removeEventListener('pagehide', saveRecord);
      window.removeEventListener('beforeunload', saveRecord);
      document.removeEventListener('visibilitychange', onVisibility);
    };
  });

  // 音量同步
  $effect(() => {
    if (audio) audio.volume = player.volume;
  });

  // 侧边栏按 Apple Music 的分组方式排：顶部单项、资料库、其他、底部账号
  //
  // 「推荐」是网易云专属页面：登录后它同时是首页，排在「搜索」前面；
  // 没登录时整项不出现——它没有内容可看，留着只会占位。
  const navTop = $derived(
    hasNetease()
      ? [
          { name: 'recommend', label: '推荐', icon: icons.hot },
          { name: 'search', label: '搜索', icon: icons.search }
        ]
      : [{ name: 'search', label: '搜索', icon: icons.search }]
  );
  // 「喜欢」存到网盘，只有网盘启用时才出现
  const navLibrary = $derived([
    ...(appCfg.s3Enabled ? [{ name: 'library', label: '喜欢', icon: icons.favorite }] : []),
    { name: 'downloads', label: '下载', icon: icons.downloadDir },
    { name: 'cloud', label: '网盘', icon: icons.cloud },
  ]);
  const navOther = [{ name: 'settings', label: '设置', icon: icons.settings }];

  const activeNav = $derived(
    ['track', 'artist', 'album', 'lyrics'].includes(router.route.name)
      ? 'search'
      : router.route.name === 'accountplaylists'
        ? 'account'
        : router.route.name
  );
</script>

<svelte:window onresize={measureNav} />

<div class="app">
  {#if customChrome}
    <TitleBar />
  {/if}
  <aside bind:this={asideEl}>
    <span
      class="nav-ind"
      class:show={indShow}
      class:ready={indReady}
      style="top: {indTop}px; height: {indH}px"
    ></span>
    <!-- macOS 使用 Overlay 标题栏：给红绿灯留位置，并让这块区域可以拖动窗口 -->
    <div class="titlebar-drag" data-tauri-drag-region aria-hidden="true"></div>
    <div class="brand">
      <span class="logo"><HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} /></span>
      <span>TinyMusic</span>
    </div>
    <nav class="side-nav">
      <!-- 必须按 name 做 key：features 异步加载后导航项会插队，
           不 key 的话 Svelte 会复用按钮，HugeiconsIcon 的 icon 属性不跟随更新，
           结果「喜欢」显示成下载图标、「下载」和「设置」都显示齿轮。 -->
      {#each navTop as n (n.name)}
        <button class:on={activeNav === n.name} onclick={() => go({ name: n.name } as never)}>
          <span class="ico"><HugeiconsIcon icon={n.icon} size={ICON_SIZE.control} /></span>{n.label}
        </button>
      {/each}
      <!-- 分组不再写标题：用一道空气把组分开，避免一条条横线把侧边栏切开 -->
      <div class="nav-sep" aria-hidden="true"></div>
      {#each navLibrary as n (n.name)}
        <button class:on={activeNav === n.name} onclick={() => go({ name: n.name } as never)}>
          <span class="ico"><HugeiconsIcon icon={n.icon} size={ICON_SIZE.control} /></span>{n.label}
        </button>
      {/each}
      <div class="nav-sep" aria-hidden="true"></div>
      {#each navOther as n (n.name)}
        <button class:on={activeNav === n.name} onclick={() => go({ name: n.name } as never)}>
          <span class="ico"><HugeiconsIcon icon={n.icon} size={ICON_SIZE.control} /></span>{n.label}
          <!-- 静默检查到新版才出现的一颗小红点，不写版本号、不闪 -->
          {#if n.name === 'settings' && updateState.version}
            <span class="nav-dot" title={`有新版本 ${updateState.version}，点进来更新`}></span>
          {/if}
        </button>
      {/each}
    </nav>
    <div class="side-foot">
      <button class="foot" class:on={activeNav === 'account'} onclick={() => go({ name: 'account' })}>
        <span class="avatar" class:logged={!!account.uid || hasNetease()}>
          {#if account.uid}
            {account.uid.slice(0, 1).toUpperCase()}
          {:else if hasNetease()}
            {(account.netease.nickname || account.netease.uid).slice(0, 1).toUpperCase()}
          {:else}
            <HugeiconsIcon icon={icons.account} size={ICON_SIZE.inline} />
          {/if}
        </span>
        <span class="foot-meta">
          <span class="foot-name">账号</span>
          <span class="foot-sub"
            >{account.uid
              ? '已登录'
              : hasNetease()
                ? account.netease.nickname || '网易云已登录'
                : '未登录'}</span
          >
        </span>
      </button>
    </div>
  </aside>

  <main bind:this={mainEl} class:no-player={!player.current}>
    {#if bootErr}<div class="err">{bootErr}</div>{/if}
    {#if router.route.name === 'search'}
      <Search />
    {:else if router.route.name === 'recommend'}
      <Recommend />
    {:else if router.route.name === 'playlist'}
      <Playlist />
    {:else if router.route.name === 'account'}
      <Account />
    {:else if router.route.name === 'accountplaylists'}
      <AccountPlaylists />
    {:else if router.route.name === 'track'}
      <Track />
    {:else if router.route.name === 'artist'}
      <Artist />
    {:else if router.route.name === 'album'}
      <Album />
    {:else if router.route.name === 'downloads'}
      <Downloads />
    {:else if router.route.name === 'library'}
      <Library />
    {:else if router.route.name === 'settings'}
      <Settings />
    {:else if router.route.name === 'cloud'}
      <Cloud />
    {/if}
  </main>

  <footer>
    <PlayerBar />
  </footer>

  {#if player.lyricsOpen && player.current}
    <Lyrics />
  {/if}

  <DownloadDialog />
  <ContextMenu />
  <Toast />

  <!-- 窗口按钮浮在所有浮层之上：歌词页全屏盖住时也关得掉窗口 -->
  {#if customChrome}
    <WindowControls />
  {/if}

  <!-- 隐藏的音频元素：真正的解码/播放器 -->
  <audio
    bind:this={audio}
    ontimeupdate={(e) => {
      const a = e.currentTarget as HTMLAudioElement;
      player.progress = a.currentTime;
      persistPlayback();
    }}
    onloadedmetadata={(e) => {
      const a = e.currentTarget as HTMLAudioElement;
      const duration = isFinite(a.duration) && a.duration > 0 ? a.duration : 0;
      if (duration > 0) player.duration = duration;

      // 恢复上次进度：等 metadata 就绪后再 seek，然后播放
      const seekTo = consumePendingSeek();
      if (seekTo != null) {
        const target = duration > 0 ? Math.min(seekTo, Math.max(0, duration - 0.5)) : seekTo;
        try {
          a.currentTime = target;
        } catch {
          /* ignore */
        }
        player.progress = target;
        a.play().catch((err) => void handlePlayError(err));
      }
      persistPlayback(true);
    }}
    onended={() => {
      onEnded();
      persistPlayback(true);
    }}
    onerror={() => {
      clearPendingSeek();
      // 挂掉的直链（缓存里的旧地址、代理 token 过期）先自动换一条再续播，
      // 只有重试过了还失败才把错误摆到播放条上。
      void recoverSource().then((retried) => {
        if (retried) return;
        const what = player.current?.provider === 's3' ? '网盘文件可能已被删除或直链过期' : '直链可能已过期';
        if (player.src) player.error = `音频加载失败（${what}）`;
        player.status = 'error';
        player.playing = false;
      });
    }}
    onplay={() => syncPlayback('playing')}
    onpause={(e) => {
      player.progress = (e.currentTarget as HTMLAudioElement).currentTime;
      syncPlayback('paused');
      persistPlayback(true);
    }}
  ></audio>
</div>
