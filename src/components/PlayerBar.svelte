<script lang="ts">
  import { fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration, isBlocked } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import {
    player,
    toggle,
    next,
    prev,
    seek,
    setVolume,
    setQuality,
    currentQualityChoices,
    refreshTrackQualities,
    playButtonLabel,
    playTrack,
    openLyrics,
    toggleQueue,
    toggleVolume,
    closeVolume,
    toggleShuffle,
    cycleRepeat,
  } from '../lib/player.svelte';
  import { requestDownload, isPending } from '../lib/download.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { go } from '../lib/router.svelte';
  import QueuePanel from './QueuePanel.svelte';

  const pending = $derived(player.current ? isPending(player.current) : false);

  let scrubbing = $state(false);
  let scrubVal = $state(0);

  // 封面加载失败时退回占位图标（网盘封面直链过期、CDN 防盗链都走这条）
  let coverFailedFor = $state('');
  const coverKey = $derived(
    player.current ? `${player.current.provider}:${player.current.id}:${player.current.cover ?? ''}` : ''
  );
  const showCover = $derived(!!player.current?.cover && coverFailedFor !== coverKey);

  const pct = $derived(
    player.duration > 0
      ? Math.min(100, ((scrubbing ? scrubVal : player.progress) / player.duration) * 100)
      : 0
  );

  const repeatLabel = $derived(
    player.repeat === 'one' ? '单曲循环' : player.repeat === 'all' ? '列表循环' : '不循环'
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

  /** 播放器里的「…」菜单：歌词 / 下载 / 音质 / 详情入口都收在这里 */
  async function openMore(e: MouseEvent) {
    const t = player.current;
    if (!t) return;
    // 列表没带 MINFO 时先探测一次，避免菜单打开后才变灰
    if (!player.qualities?.known) {
      await refreshTrackQualities(t);
    }
    const downloading = isPending(t);
    openMenuAt(e.clientX, e.clientY, [
      { label: '查看歌词', icon: icons.lyric, run: openLyrics },
      {
        label: downloading ? '正在下载…' : '下载当前曲目',
        icon: icons.download,
        disabled: downloading || isBlocked(t.availability),
        run: () => void requestDownload([t]),
      },
      { sep: true },
      {
        label: '音质',
        icon: icons.quality,
        submenu: currentQualityChoices().map((o) => ({
          label: o.label,
          selected: o.current,
          disabled: o.disabled,
          run: () => void setQuality(o.value),
        })),
      },
      { sep: true },
      { label: '歌曲详情', icon: icons.disc, run: () => go({ name: 'track', track: t }) },
      ...(t.album_id
        ? [
            {
              label: '打开专辑',
              icon: icons.disc,
              run: () =>
                go({ name: 'album', provider: t.provider, albumId: t.album_id, title: t.album }),
            },
          ]
        : []),
      ...(t.artist_id
        ? [
            {
              label: '打开歌手',
              icon: icons.account,
              run: () =>
                go({ name: 'artist', provider: t.provider, artistId: t.artist_id, title: t.artist }),
            },
          ]
        : []),
    ]);
  }

  const volumeLabel = $derived(
    player.volume === 0 ? '已静音' : `音量 ${Math.round(player.volume * 100)}%`
  );
  const volumeIcon = $derived(
    player.volume === 0
      ? icons.volumeOff
      : player.volume <= 0.45
        ? icons.volumeLow
        : icons.volumeHigh
  );

  /** 音量滑杆展开时，鼠标短暂离开不要立刻收起 */
  let volLeaveTimer: ReturnType<typeof setTimeout> | undefined;
  let volDragging = $state(false);

  function cancelVolLeave() {
    clearTimeout(volLeaveTimer);
  }

  function scheduleVolClose() {
    if (volDragging) return;
    cancelVolLeave();
    volLeaveTimer = setTimeout(() => closeVolume(), 140);
  }

  let volTrack = $state<HTMLElement | null>(null);
  let volStartX = 0;
  let volMoved = false;

  function setVolFromPointer(e: PointerEvent) {
    if (!volTrack) return;
    const r = volTrack.getBoundingClientRect();
    // 左右各留 5px 给滑块圆点，拖到两端时不会被容器切掉
    const pad = 5;
    const usable = r.width - pad * 2;
    if (usable <= 0) return;
    setVolume(Math.max(0, Math.min(1, (e.clientX - r.left - pad) / usable)));
  }

  function volDown(e: PointerEvent) {
    volDragging = true;
    volMoved = false;
    volStartX = e.clientX;
    cancelVolLeave();
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
  }

  function volMove(e: PointerEvent) {
    if (!volDragging) return;
    // 必须真的拖动了才改音量，单纯按一下不会跳变
    if (!volMoved && Math.abs(e.clientX - volStartX) < 2) return;
    volMoved = true;
    setVolFromPointer(e);
  }

  function volUp() {
    volDragging = false;
    if (!document.querySelector('.volctl:hover')) scheduleVolClose();
  }

  function volKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 0.1 : 0.05;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') {
      e.preventDefault();
      setVolume(Math.max(0, player.volume - step));
    } else if (e.key === 'ArrowRight' || e.key === 'ArrowUp') {
      e.preventDefault();
      setVolume(Math.min(1, player.volume + step));
    } else if (e.key === 'Home') {
      e.preventDefault();
      setVolume(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setVolume(1);
    }
  }
</script>

{#if player.current}
<QueuePanel />

<div
  class="bar"
  role="group"
  aria-label="播放器"
  transition:fly={{ y: 96, duration: 280, opacity: 0 }}
  oncontextmenu={(e) => {
    e.preventDefault();
    e.stopPropagation();
    void openMore(e);
  }}
>
  <div class="transport">
    <button
      class="tb"
      class:on={player.shuffle}
      onclick={toggleShuffle}
      title={player.shuffle ? '关闭随机播放' : '开启随机播放'}
      aria-label="随机播放"
      aria-pressed={player.shuffle}
    >
      <HugeiconsIcon icon={icons.shuffle} size={16} />
    </button>

    <button
      class="tb"
      onclick={prev}
      disabled={!player.queue.length}
      title="上一首"
      aria-label="上一首"
    >
      <HugeiconsIcon icon={icons.previous} size={ICON_SIZE.control} />
    </button>

    <button
      class="tb main"
      onclick={onMain}
      disabled={!player.current || player.status === 'loading'}
      title={playButtonLabel()}
      aria-label={playButtonLabel()}
    >
      {#if player.status === 'loading'}
        <span class="spin"><HugeiconsIcon icon={icons.loading} size={22} /></span>
      {:else if player.status === 'playing'}
        <HugeiconsIcon icon={icons.pause} size={22} />
      {:else}
        <HugeiconsIcon icon={icons.play} size={22} />
      {/if}
    </button>

    <button
      class="tb"
      onclick={next}
      disabled={!player.queue.length}
      title="下一首"
      aria-label="下一首"
    >
      <HugeiconsIcon icon={icons.next} size={ICON_SIZE.control} />
    </button>

    <button
      class="tb"
      class:on={player.repeat !== 'off'}
      onclick={cycleRepeat}
      title={`循环：${repeatLabel}（点击切换）`}
      aria-label={`循环方式：${repeatLabel}`}
      aria-pressed={player.repeat !== 'off'}
    >
      {#key player.repeat}
        <HugeiconsIcon
          icon={player.repeat === 'one' ? icons.repeatOne : icons.repeat}
          size={16}
        />
      {/key}
    </button>
  </div>

  <div class="now" role="group" aria-label="当前播放">
    <button
      class="coverbtn"
      onclick={openLyrics}
      disabled={!player.current}
      title={player.current ? '查看歌词' : '还没有播放中的歌曲'}
      aria-label="打开歌词页"
    >
      {#if showCover}
        <img
          src={player.current?.cover ?? ''}
          alt=""
          onerror={() => {
            coverFailedFor = coverKey;
          }}
        />
      {:else}
        <span class="ph" aria-hidden="true">
          <HugeiconsIcon icon={icons.music} size={ICON_SIZE.control} />
        </span>
      {/if}
    </button>

    <div class="meta">
      <div class="t">
        {player.current?.title ?? '未在播放'}
        {#if player.actualLabel}
          <span class="qchip" class:degraded={player.degraded}>{player.actualLabel}</span>
        {/if}
      </div>
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

    <div class="now-progress" class:scrubbing>
      <input
        type="range"
        min="0"
        max={Math.max(1, Math.floor(player.duration))}
        value={Math.floor(scrubbing ? scrubVal : player.progress)}
        oninput={onScrubInput}
        onchange={onScrubCommit}
        style="--p:{pct}%"
        disabled={!player.current}
        aria-label="播放进度"
        aria-valuetext={`${fmtDuration(scrubbing ? scrubVal : player.progress)} / ${fmtDuration(player.duration)}`}
      />
      <span class="seek-tip">
        {fmtDuration(scrubbing ? scrubVal : player.progress)} / {fmtDuration(player.duration)}
      </span>
    </div>
  </div>

  <div class="utils">
    <button
      class="ub"
      onclick={openLyrics}
      disabled={!player.current}
      title="歌词"
      aria-label="歌词"
    >
      <HugeiconsIcon icon={icons.lyric} size={ICON_SIZE.control} />
    </button>

    <button
      class="ub queue"
      class:on={player.queueOpen}
      onclick={toggleQueue}
      title={`播放列表（${player.queue.length} 首）`}
      aria-label="播放列表"
      aria-expanded={player.queueOpen}
    >
      <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.control} />
      {#if player.queue.length}<span class="count">{player.queue.length}</span>{/if}
    </button>

    <div
      class="volctl"
      class:open={player.volumeOpen}
      role="group"
      aria-label="音量"
      onmouseenter={cancelVolLeave}
      onmouseleave={scheduleVolClose}
    >
      <div class="vol-slide">
        <div
          class="vol-track"
          class:dragging={volDragging}
          bind:this={volTrack}
          role="slider"
          tabindex={player.volumeOpen ? 0 : -1}
          aria-label="音量"
          aria-valuemin="0"
          aria-valuemax="100"
          aria-valuenow={Math.round(player.volume * 100)}
          aria-valuetext={`${Math.round(player.volume * 100)}%`}
          onpointerdown={volDown}
          onpointermove={volMove}
          onpointerup={volUp}
          onpointercancel={volUp}
          onkeydown={volKey}
        >
          <span class="vol-rail">
            <span class="vol-fill" style="width:{player.volume * 100}%"></span>
            <span class="vol-thumb" style="left:{player.volume * 100}%"></span>
          </span>
        </div>
      </div>
      <button
        class="ub volbtn"
        class:on={player.volumeOpen}
        onclick={toggleVolume}
        title={volumeLabel}
        aria-label={volumeLabel}
        aria-expanded={player.volumeOpen}
      >
        {#key volumeIcon}
          <HugeiconsIcon icon={volumeIcon} size={ICON_SIZE.control} />
        {/key}
      </button>
    </div>
  </div>
</div>
{/if}
