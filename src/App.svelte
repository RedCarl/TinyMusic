<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from './lib/icons';
  import { router, go } from './lib/router.svelte';
  import { api } from './lib/api';
  import { player, bindAudio, restoreVolume, onEnded } from './lib/player.svelte';
  import PlayerBar from './components/PlayerBar.svelte';
  import Search from './routes/Search.svelte';
  import Playlist from './routes/Playlist.svelte';
  import Account from './routes/Account.svelte';
  import Track from './routes/Track.svelte';
  import Artist from './routes/Artist.svelte';
  import Album from './routes/Album.svelte';
  import Downloads from './routes/Downloads.svelte';
  import Library from './routes/Library.svelte';
  import Settings from './routes/Settings.svelte';

  let audio = $state<HTMLAudioElement | null>(null);
  let mainEl = $state<HTMLElement | null>(null);

  // 路由变化时把主区域滚回顶部
  $effect(() => {
    void router.route;
    mainEl?.scrollTo({ top: 0 });
  });
  let bootErr = $state('');

  onMount(() => {
    restoreVolume();
    api
      .providerFeatures('kuwo')
      .then((f) => (features = f))
      .catch(() => (features = { favorites: false }));
    // 让播放/下载用的是后端保存的音质设置
    import('./lib/api').then(({ api }) =>
      api
        .settingsGet()
        .then((s) => localStorage.setItem('quality', s.default_quality))
        .catch(() => {})
    );
    bindAudio((action, value) => {
      const a = audio;
      if (!a) return;
      switch (action) {
        case 'load':
          a.src = player.src;
          a.load();
          break;
        case 'play':
          a.play().catch((e) => {
            player.error = '播放失败：' + String(e);
          });
          break;
        case 'pause':
          a.pause();
          break;
        case 'seek':
          a.currentTime = value ?? 0;
          break;
      }
    });
  });

  // 音量同步
  $effect(() => {
    if (audio) audio.volume = player.volume;
  });

  // 「喜欢」只在平台提供账号级喜欢集合时才出现
  let features = $state<{ favorites: boolean }>({ favorites: false });

  const nav = $derived([
    { name: 'search', label: '搜索', icon: icons.search },
    ...(features.favorites ? [{ name: 'library', label: '喜欢', icon: icons.favorite }] : []),
    { name: 'downloads', label: '下载', icon: icons.download },
    { name: 'settings', label: '设置', icon: icons.settings },
  ]);

  const activeNav = $derived(
    ['track', 'artist', 'album'].includes(router.route.name) ? 'search' : router.route.name
  );
</script>

<div class="app">
  <aside>
    <div class="brand">
      <span class="logo"><HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} /></span>
      <span>TinyMusic</span>
    </div>
    <nav>
      {#each nav as n}
        <button class:on={activeNav === n.name} onclick={() => go({ name: n.name } as never)}>
          <span class="ico"><HugeiconsIcon icon={n.icon} size={ICON_SIZE.control} /></span>{n.label}
        </button>
      {/each}
    </nav>
    <button class="foot" class:on={activeNav === 'account'} onclick={() => go({ name: 'account' })}>
      <HugeiconsIcon icon={icons.account} size={ICON_SIZE.inline} />
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
    {/if}
  </main>

  <footer>
    <PlayerBar />
  </footer>

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
      player.playing = false;
    }}
    onplay={() => (player.playing = true)}
    onpause={() => (player.playing = false)}
  ></audio>
</div>
