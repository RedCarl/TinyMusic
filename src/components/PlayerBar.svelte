<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import {
    player,
    toggle,
    next,
    prev,
    seek,
    setVolume,
    setQuality,
    QUALITY_OPTIONS,
    playButtonLabel,
    playTrack,
    openLyrics,
    toggleQueue,
    toggleVolume,
    closeVolume,
    toggleMute,
    PLAY_MODES,
  } from '../lib/player.svelte';
  import { requestDownload, isPending } from '../lib/download.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import QueuePanel from './QueuePanel.svelte';

  const pending = $derived(player.current ? isPending(player.current) : false);
  const modeMeta = $derived(PLAY_MODES.find((m) => m.value === player.mode) ?? PLAY_MODES[0]);

  let scrubbing = $state(false);
  let scrubVal = $state(0);

  const pct = $derived(
    player.duration > 0
      ? Math.min(100, ((scrubbing ? scrubVal : player.progress) / player.duration) * 100)
      : 0
  );

  function onScrubInput(e: Event) {
    scrubbing = true;
    scrubVal = Number((e.target as HTMLInputElement).value);
  }
  function onScrubCommit(e: Event) {
    const v = Number((e.target as HTMLInputElement).value);
    scrubbing = false;
    seek(v);
  }

  function onMain() {
    if (player.status === 'error' && player.current) {
      void playTrack(player.current);
      return;
    }
    toggle();
  }

  function downloadCurrent() {
    if (player.current) void requestDownload([player.current]);
  }

  const volumeLabel = $derived(player.volume === 0 ? '取消静音' : '调节音量');
</script>

<QueuePanel />

<div class="bar">
  <div
    class="left"
    role="group"
    oncontextmenu={(e) => {
      if (!player.current) return;
      e.preventDefault();
      e.stopPropagation();
      openMenuAt(e.clientX, e.clientY, [
        { label: '查看歌词', icon: icons.lyric, run: openLyrics },
        {
          label: player.status === 'playing' ? '暂停' : '继续播放',
          icon: player.status === 'playing' ? icons.pause : icons.play,
          disabled: player.status === 'loading',
          run: toggle,
        },
        { sep: true },
        {
          label: '下载当前曲目（含歌词与封面）',
          icon: icons.download,
          run: () => {
            if (player.current) void requestDownload([player.current]);
          },
        },
        { label: '下一首', icon: icons.next, run: next },
      ]);
    }}
  >
    <button
      class="coverbtn"
      onclick={openLyrics}
      disabled={!player.current}
      title={player.current ? '查看歌词' : '还没有播放中的歌曲'}
      aria-label="打开歌词页"
    >
      {#if player.current?.cover}
        <img src={player.current.cover} alt="" />
      {:else}
        <span class="ph" aria-hidden="true">
          <HugeiconsIcon icon={icons.music} size={ICON_SIZE.control} />
        </span>
      {/if}
      {#if player.current}
        <span class="coverhint" aria-hidden="true">
          <HugeiconsIcon icon={icons.lyric} size={ICON_SIZE.inline} />
        </span>
      {/if}
    </button>
    <div class="meta">
      <div class="t">{player.current?.title ?? '未在播放'}</div>
      <div class="a">
        {#if player.error}
          <span class="err">{player.error.includes('会员') || player.error.includes('unavailable')
            ? '需要会员，未登录不可用'
            : player.error}</span>
        {:else if player.status === 'loading'}
          正在取流…
        {:else}
          {player.current?.artist ?? ''}
        {/if}
      </div>
    </div>
  </div>

  <div class="center">
    <div class="ctrls">
      <button class="c" onclick={prev} title="上一首" aria-label="上一首" disabled={!player.queue.length}>
        <HugeiconsIcon icon={icons.previous} size={ICON_SIZE.control} />
      </button>
      <button
        class="c main"
        onclick={onMain}
        disabled={!player.current || player.status === 'loading'}
        title={playButtonLabel()}
        aria-label={playButtonLabel()}
      >
        {#if player.status === 'loading'}
          <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.control} /></span>
        {:else if player.status === 'playing'}
          <HugeiconsIcon icon={icons.pause} size={ICON_SIZE.control} />
        {:else}
          <HugeiconsIcon icon={icons.play} size={ICON_SIZE.control} />
        {/if}
      </button>
      <button class="c" onclick={next} title="下一首" aria-label="下一首" disabled={!player.queue.length}>
        <HugeiconsIcon icon={icons.next} size={ICON_SIZE.control} />
      </button>
    </div>
    <div class="seek">
      <span>{fmtDuration(scrubbing ? scrubVal : player.progress)}</span>
      <input
        type="range"
        min="0"
        max={Math.max(1, Math.floor(player.duration))}
        value={Math.floor(scrubbing ? scrubVal : player.progress)}
        oninput={onScrubInput}
        onchange={onScrubCommit}
        style="--p:{pct}%"
      />
      <span>{fmtDuration(player.duration)}</span>
    </div>
  </div>

  <div class="right">
    <select
      class="quality"
      value={player.quality}
      onchange={(e) => setQuality((e.target as HTMLSelectElement).value as never)}
      title="播放音质"
      aria-label="播放音质"
    >
      {#each QUALITY_OPTIONS as o (o.value)}
        <option value={o.value}>{o.label}</option>
      {/each}
    </select>

    <button
      class="c qbtn"
      class:on={player.queueOpen}
      onclick={toggleQueue}
      title={`播放列表（${modeMeta.label}）`}
      aria-label="播放列表"
    >
      <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.control} />
      {#if player.queue.length}<span class="count">{player.queue.length}</span>{/if}
    </button>

    <button
      class="c"
      onclick={downloadCurrent}
      disabled={!player.current || pending}
      title={pending ? '正在下载…' : '下载当前曲目（含歌词与封面）'}
      aria-label="下载当前曲目"
    >
      {#if pending}
        <span class="spin"><HugeiconsIcon icon={icons.loading} size={ICON_SIZE.inline} /></span>
      {:else}
        <HugeiconsIcon icon={icons.download} size={ICON_SIZE.inline} />
      {/if}
    </button>

    <button
      class="c volbtn"
      class:on={player.volumeOpen}
      onclick={toggleVolume}
      title={volumeLabel}
      aria-label={volumeLabel}
      aria-expanded={player.volumeOpen}
    >
      <HugeiconsIcon
        icon={player.volume === 0 ? icons.volumeOff : icons.volume}
        size={ICON_SIZE.inline}
      />
    </button>
  </div>
</div>

{#if player.volumeOpen}
  <div class="vol-mask" role="presentation" onclick={closeVolume}></div>
  <div class="vol-panel" role="dialog" aria-label="音量">
    <button
      class="vol-mute"
      onclick={toggleMute}
      title={player.volume === 0 ? '取消静音' : '静音'}
      aria-label={player.volume === 0 ? '取消静音' : '静音'}
    >
      <HugeiconsIcon
        icon={player.volume === 0 ? icons.volumeOff : icons.volume}
        size={ICON_SIZE.inline}
      />
    </button>
    <input
      type="range"
      min="0"
      max="1"
      step="0.01"
      value={player.volume}
      style="--p:{Math.round(player.volume * 100)}%"
      oninput={(e) => setVolume(Number((e.target as HTMLInputElement).value))}
      aria-label="音量"
    />
    <span class="volval">{Math.round(player.volume * 100)}%</span>
  </div>
{/if}
