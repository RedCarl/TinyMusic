<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { fmtDuration } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { api } from '../lib/api';
  import {
    player,
    toggle,
    next,
    prev,
    seek,
    setVolume,
    setQuality,
    QUALITY_OPTIONS
  } from '../lib/player.svelte';

  let downloading = $state(false);
  let dlMsg = $state('');

  async function downloadCurrent() {
    const t = player.current;
    if (!t || downloading) return;
    downloading = true;
    dlMsg = '';
    try {
      await api.downloadStart(t, player.quality, (p) => {
        if (p.total && p.total > 0) {
          dlMsg = `${Math.round((p.downloaded / p.total) * 100)}%`;
        }
      });
      dlMsg = '已下载';
    } catch (e) {
      dlMsg = '下载失败';
      player.error = String(e);
    } finally {
      downloading = false;
      setTimeout(() => (dlMsg = ''), 2500);
    }
  }

  let scrubbing = $state(false);
  let scrubVal = $state(0);

  const pct = $derived(
    player.duration > 0 ? Math.min(100, ((scrubbing ? scrubVal : player.progress) / player.duration) * 100) : 0
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
</script>

<div class="bar">
  <div class="left">
    {#if player.current?.cover}
      <img src={player.current.cover} alt="" />
    {:else}
      <div class="ph" aria-hidden="true">
        <HugeiconsIcon icon={icons.music} size={ICON_SIZE.control} />
      </div>
    {/if}
    <div class="meta">
      <div class="t">{player.current?.title ?? '未在播放'}</div>
      <div class="a">
        {#if player.error}
          <span class="err">{player.error.includes('会员') || player.error.includes('unavailable')
            ? '需要会员，未登录不可用'
            : player.error}</span>
        {:else}
          {player.current?.artist ?? ''}
        {/if}
      </div>
    </div>
  </div>

  <div class="center">
    <div class="ctrls">
      <button class="c" onclick={prev} title="上一首" aria-label="上一首">
        <HugeiconsIcon icon={icons.previous} size={ICON_SIZE.control} />
      </button>
      <button
        class="c main"
        onclick={toggle}
        disabled={!player.current || player.loading}
        aria-label={player.playing ? '暂停' : '播放'}
      >
        <HugeiconsIcon
          icon={player.playing ? icons.pause : icons.play}
          size={ICON_SIZE.control}
        />
      </button>
      <button class="c" onclick={next} title="下一首" aria-label="下一首">
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
      class="c"
      onclick={downloadCurrent}
      disabled={!player.current || downloading}
      title={dlMsg || '下载当前曲目（按当前音质）'}
      aria-label="下载当前曲目"
    >
      <HugeiconsIcon icon={icons.download} size={ICON_SIZE.inline} />
    </button>

    <span class="vol" aria-hidden="true">
      <HugeiconsIcon icon={icons.volume} size={ICON_SIZE.inline} />
    </span>
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
    {#if dlMsg}<span class="dlmsg">{dlMsg}</span>{/if}
  </div>
</div>
