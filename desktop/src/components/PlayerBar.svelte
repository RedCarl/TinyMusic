<script lang="ts">
  import { fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import MorphIcon from './MorphIcon.svelte';
  import { coverUrl, fmtDuration, isBlocked } from '../lib/api';
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
    actualLabelText,
    playTrack,
    openLyrics,
    toggleQueue,
    toggleVolume,
    closeVolume,
    closeQueue,
    toggleShuffle,
    cycleRepeat,
  } from '../lib/player.svelte';
  import { equalizer, toggleEqualizerPanel, closeEqualizer } from '../lib/equalizer.svelte';
  import { requestDownload, isPending } from '../lib/download.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { go } from '../lib/router.svelte';
  import { copyText, showToast } from '../lib/toast.svelte';
  import { appCfg } from '../lib/config.svelte';
  import { isFavorite, ensureFavorite, toggleFavorite } from '../lib/favorite.svelte';
  import { t } from '../lib/i18n.svelte';
  import QueuePanel from './QueuePanel.svelte';

  const pending = $derived(player.current ? isPending(player.current) : false);
  const liked = $derived(player.current ? isFavorite(player.current) : false);

  // 换歌后查一次收藏状态，红心跟着当前曲目走
  $effect(() => {
    const cur = player.current;
    if (cur && appCfg.s3Enabled) void ensureFavorite(cur);
  });

  async function toggleFav() {
    const cur = player.current;
    if (!cur) return;
    try {
      await toggleFavorite(cur);
    } catch (e) {
      showToast(String(e));
    }
  }

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

  /** 播放条上标注这段声音的真实来源：本地文件 / 网盘 / 网易 / 酷我 */
  const sourceBadge = $derived.by(() => {
    const via = player.via;
    const provider = player.current?.provider ?? '';
    if (via === 'local') return t('本地');
    if (via === 's3' || provider === 's3') return t('网盘');
    if (via.startsWith('kuwo-bridge')) return t('酷我');
    if (provider === 'netease') return t('网易');
    if (provider === 'kuwo') return t('酷我');
    return '';
  });

  const repeatLabel = $derived(
    player.repeat === 'one' ? t('单曲循环') : player.repeat === 'all' ? t('列表循环') : t('不循环')
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
    const track = player.current;
    if (!track) return;
    // 列表没带 MINFO 时先探测一次，避免菜单打开后才变灰
    if (!player.qualities?.known) {
      await refreshTrackQualities(track);
    }
    const downloading = isPending(track);
    openMenuAt(e.clientX, e.clientY, [
      { label: t('查看歌词'), icon: icons.lyric, run: openLyrics },
      {
        label: downloading ? t('正在下载…') : t('下载当前曲目'),
        icon: icons.download,
        disabled: downloading || isBlocked(track.availability),
        run: () => void requestDownload([track]),
      },
      { sep: true },
      {
        label: t('音质'),
        icon: icons.quality,
        submenu: currentQualityChoices().map((o) => ({
          label: t(o.label),
          selected: o.current,
          disabled: o.disabled,
          run: () => void setQuality(o.value),
        })),
      },
      { sep: true },
      { label: t('歌曲详情'), icon: icons.disc, run: () => go({ name: 'track', track }) },
      ...(track.album_id
        ? [
            {
              label: t('打开专辑'),
              icon: icons.disc,
              run: () =>
                go({
                  name: 'album',
                  provider: track.provider,
                  albumId: track.album_id,
                  title: track.album,
                }),
            },
          ]
        : []),
      ...(track.artist_id
        ? [
            {
              label: t('打开歌手'),
              icon: icons.account,
              run: () =>
                go({
                  name: 'artist',
                  provider: track.provider,
                  artistId: track.artist_id,
                  title: track.artist,
                }),
            },
          ]
        : []),
    ]);
  }

  const volumeLabel = $derived(
    player.volume === 0
      ? t('已静音')
      : t('音量 {n}%', { n: Math.round(player.volume * 100) })
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

  /** 均衡器面板和播放列表 / 音量浮层互斥 */
  function openEqPanel() {
    closeQueue();
    closeVolume();
    toggleEqualizerPanel();
  }
</script>

{#if player.current}
<QueuePanel />

<div
  class="bar"
  role="group"
  aria-label={t('播放器')}
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
      title={player.shuffle ? t('关闭随机播放') : t('开启随机播放')}
      aria-label={t('随机播放')}
      aria-pressed={player.shuffle}
    >
      <HugeiconsIcon icon={icons.shuffle} size={16} />
    </button>

    <button
      class="tb"
      onclick={prev}
      disabled={!player.queue.length}
      title={t('上一首')}
      aria-label={t('上一首')}
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
      {:else}
        <MorphIcon icon={player.status === 'playing' ? icons.pause : icons.play} size={22} />
      {/if}
    </button>

    <button
      class="tb"
      onclick={next}
      disabled={!player.queue.length}
      title={t('下一首')}
      aria-label={t('下一首')}
    >
      <HugeiconsIcon icon={icons.next} size={ICON_SIZE.control} />
    </button>

    <button
      class="tb"
      class:on={player.repeat !== 'off'}
      onclick={cycleRepeat}
      title={t('循环：{label}（点击切换）', { label: repeatLabel })}
      aria-label={t('循环方式：{label}', { label: repeatLabel })}
      aria-pressed={player.repeat !== 'off'}
    >
      <MorphIcon icon={player.repeat === 'one' ? icons.repeatOne : icons.repeat} size={16} />
    </button>
  </div>

  <div class="now" role="group" aria-label={t('当前播放')}>
    <button
      class="coverbtn"
      onclick={openLyrics}
      disabled={!player.current}
      title={player.current ? t('查看歌词') : t('还没有播放中的歌曲')}
      aria-label={t('打开歌词页')}
    >
      {#if showCover}
        <img
          src={coverUrl(player.current?.cover, 500)}
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
        {#if player.current && sourceBadge}<span class="srcchip">{sourceBadge}</span>{/if}
        {player.current?.title ?? t('未在播放')}
        {#if actualLabelText()}
          <span class="qchip" class:degraded={player.degraded}>{actualLabelText()}</span>
        {/if}
      </div>
      <div class="a">
        {#if player.error}
          <button
            type="button"
            class="err err-link"
            title={t('{err}（点击复制）', { err: player.error })}
            onclick={() => copyText(player.error, t('已复制错误信息'))}
          >
            {player.error.includes('会员') || player.error.includes('unavailable')
              ? t('需要会员，未登录不可用')
              : player.error}
          </button>
        {:else if player.status === 'loading'}
          {t('正在取流…')}
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
        aria-label={t('播放进度')}
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
      class:on={liked}
      onclick={toggleFav}
      disabled={!player.current || !appCfg.s3Enabled}
      title={!player.current
        ? t('还没有播放中的歌曲')
        : liked
          ? t('取消喜欢')
          : t('喜欢（收藏到网盘）')}
      aria-label={liked ? t('取消喜欢') : t('喜欢')}
      aria-pressed={liked}
    >
      <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.control} />
    </button>

    <button
      class="ub"
      onclick={openLyrics}
      disabled={!player.current}
      title={t('歌词')}
      aria-label={t('歌词')}
    >
      <HugeiconsIcon icon={icons.lyric} size={ICON_SIZE.control} />
    </button>

    <button
      class="ub queue"
      class:on={player.queueOpen}
      onclick={() => {
        closeEqualizer();
        toggleQueue();
      }}
      title={t('播放列表（{n} 首）', { n: player.queue.length })}
      aria-label={t('播放列表')}
      aria-expanded={player.queueOpen}
    >
      <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.control} />
      {#if player.queue.length}<span class="count">{player.queue.length}</span>{/if}
    </button>

    <button
      class="ub"
      class:on={equalizer.enabled}
      onclick={openEqPanel}
      title={equalizer.enabled ? t('均衡器（已开启）') : t('均衡器')}
      aria-label={t('均衡器')}
      aria-expanded={equalizer.open}
    >
      <HugeiconsIcon icon={icons.equalizer} size={ICON_SIZE.control} />
    </button>

    <div
      class="volctl"
      class:open={player.volumeOpen}
      role="group"
      aria-label={t('音量')}
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
          aria-label={t('音量')}
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
        onclick={() => {
          closeEqualizer();
          toggleVolume();
        }}
        title={volumeLabel}
        aria-label={volumeLabel}
        aria-expanded={player.volumeOpen}
      >
        <MorphIcon icon={volumeIcon} size={ICON_SIZE.control} />
      </button>
    </div>
  </div>
</div>
{/if}

<style>
  /* 播放条上的报错是单行，放不下复制按钮：点一下就把原始报错复制走 */
  .err-link {
    display: block;
    max-width: 100%;
    padding: 0;
    overflow: hidden;
    color: var(--warn);
    font-size: 11px;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: copy;
  }

  .err-link:hover {
    text-decoration: underline dotted;
  }
</style>
