<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { listen } from '@tauri-apps/api/event';
  import { icons, ICON_SIZE } from './lib/icons';
  import { router, go } from './lib/router.svelte';
  import { api } from './lib/api';
  import {
    player,
    bindAudio,
    restoreVolume,
    onEnded,
    toggle,
    next,
    prev,
    syncPlayback,
  } from './lib/player.svelte';
  import PlayerBar from './components/PlayerBar.svelte';
  import DownloadDialog from './components/DownloadDialog.svelte';
  import ContextMenu from './components/ContextMenu.svelte';
  import Toast from './components/Toast.svelte';
  import Search from './routes/Search.svelte';
  import Playlist from './routes/Playlist.svelte';
  import Account from './routes/Account.svelte';
  import Track from './routes/Track.svelte';
  import Artist from './routes/Artist.svelte';
  import Album from './routes/Album.svelte';
  import Downloads from './routes/Downloads.svelte';
  import Library from './routes/Library.svelte';
  import Settings from './routes/Settings.svelte';
  import Cloud from './routes/Cloud.svelte';
  import Lyrics from './routes/Lyrics.svelte';
  import { appCfg, refreshAppCfg } from './lib/config.svelte';

  let audio = $state<HTMLAudioElement | null>(null);
  let mainEl = $state<HTMLElement | null>(null);

  // 路由变化时把主区域滚回顶部。
  // 搜索页例外：它自己保存/恢复滚动位置，不能在这里重置。
  $effect(() => {
    const name = router.route.name;
    if (name === 'search') return;
    mainEl?.scrollTo({ top: 0 });
  });

  let bootErr = $state('');

  onMount(() => {
    restoreVolume();
    // 让播放/下载用的是后端保存的音质设置
    import('./lib/api').then(({ api: a }) =>
      a
        .settingsGet()
        .then((s) => {
          const q = s.default_quality === 'lossless' ? 'super' : s.default_quality;
          localStorage.setItem('quality', q);
          // 网盘启用时把云端的「喜欢」拉回来合并
          if (s.s3_enabled && s.s3_bucket) a.favoritesPull().catch(() => {});
        })
        .catch(() => {})
    );
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
          el.play().catch((e) => {
            player.error = '播放失败：' + String(e);
            player.status = 'error';
            player.playing = false;
          });
          break;
        case 'pause':
          el.pause();
          break;
        case 'seek':
          el.currentTime = value ?? 0;
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

    return () => unlisten?.();
  });

  // 音量同步
  $effect(() => {
    if (audio) audio.volume = player.volume;
  });

  // 「喜欢」存到网盘，只有网盘启用时才出现
  const nav = $derived([
    { name: 'search', label: '搜索', icon: icons.search },
    ...(appCfg.s3Enabled ? [{ name: 'library', label: '喜欢', icon: icons.favorite }] : []),
    { name: 'downloads', label: '下载', icon: icons.downloadDir },
    { name: 'cloud', label: '网盘', icon: icons.cloud },
    { name: 'settings', label: '设置', icon: icons.settings },
  ]);

  const activeNav = $derived(
    ['track', 'artist', 'album', 'lyrics'].includes(router.route.name) ? 'search' : router.route.name
  );
</script>

<div class="app">
  <aside>
    <div class="brand">
      <span class="logo"><HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} /></span>
      <span>TinyMusic</span>
    </div>
    <nav>
      <!-- 必须按 name 做 key：features 异步加载后导航项会插队，
           不 key 的话 Svelte 会复用按钮，HugeiconsIcon 的 icon 属性不跟随更新，
           结果「喜欢」显示成下载图标、「下载」和「设置」都显示齿轮。 -->
      {#each nav as n (n.name)}
        <button class:on={activeNav === n.name} onclick={() => go({ name: n.name } as never)}>
          <span class="ico"><HugeiconsIcon icon={n.icon} size={ICON_SIZE.control} /></span>{n.label}
        </button>
      {/each}
    </nav>
    <button class="foot" class:on={activeNav === 'account'} onclick={() => go({ name: 'account' })}>
      <span class="ico"><HugeiconsIcon icon={icons.account} size={ICON_SIZE.control} /></span>
      <span>账号</span>
    </button>
  </aside>

  <main bind:this={mainEl}>
    {#if bootErr}<div class="err">{bootErr}</div>{/if}
    {#if router.route.name === 'search'}
      <Search />
    {:else if router.route.name === 'playlist'}
      <Playlist />
    {:else if router.route.name === 'account'}
      <Account />
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

  <!-- 隐藏的音频元素：真正的解码/播放器 -->
  <audio
    bind:this={audio}
    ontimeupdate={(e) => {
      const a = e.currentTarget as HTMLAudioElement;
      player.progress = a.currentTime;
    }}
    onloadedmetadata={(e) => {
      const a = e.currentTarget as HTMLAudioElement;
      if (isFinite(a.duration) && a.duration > 0) player.duration = a.duration;
    }}
    onended={() => onEnded()}
    onerror={() => {
      if (player.src) player.error = '音频加载失败（直链可能已过期）';
      player.status = 'error';
      player.playing = false;
    }}
    onplay={() => syncPlayback('playing')}
    onpause={() => syncPlayback('paused')}
  ></audio>
</div>
