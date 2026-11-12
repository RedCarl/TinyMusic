<script lang="ts">
  import { fade, fly } from 'svelte/transition';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import EqBand from './EqBand.svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { closeQueue, closeVolume } from '../lib/player.svelte';
  import { showToast } from '../lib/toast.svelte';
  import {
    EQ_BANDS,
    EQ_MAX_DB,
    EQ_MIN_DB,
    EQ_PRESETS,
    closeEqualizer,
    currentPresetName,
    equalizer,
    formatDb,
    resetEqualizer,
    setBandGain,
    setEqualizerEnabled,
    setPreamp,
    setPreset
  } from '../lib/equalizer.svelte';

  // 面板打开时让播放列表 / 音量的浮层先收起，避免叠在一起
  $effect(() => {
    if (!equalizer.open) return;
    closeQueue();
    closeVolume();
  });

  const preampPct = $derived(((equalizer.preamp - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB)) * 100);

  function toggleEnabled(input: HTMLInputElement) {
    const checked = input.checked;
    if (!setEqualizerEnabled(checked) && checked) {
      // 失败时把 DOM 勾选状态退回去，否则开关会停在「已开」但实际没生效
      input.checked = false;
      showToast('当前系统不支持均衡器，已关闭');
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && equalizer.open) {
      e.preventDefault();
      closeEqualizer();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if equalizer.open}
  <div
    class="eq-mask"
    role="presentation"
    onclick={closeEqualizer}
    transition:fade={{ duration: 140 }}
  ></div>

  <div
    class="eq-panel"
    role="dialog"
    aria-label="均衡器"
    transition:fly={{ y: 10, duration: 180, opacity: 0 }}
  >
    <div class="eq-head">
      <span class="eq-icon" aria-hidden="true">
        <HugeiconsIcon icon={icons.equalizer} size={ICON_SIZE.control} />
      </span>
      <div class="eq-heading">
        <div class="eq-title">
          均衡器
          <span class="eq-badge">10 段</span>
        </div>
        <div class="eq-sub">32 Hz – 16 kHz · 实时作用于播放</div>
      </div>

      <label
        class="eq-switch"
        class:disabled={equalizer.unsupported}
        title={equalizer.unsupported ? '当前系统不支持音效处理' : equalizer.enabled ? '关闭均衡器' : '开启均衡器'}
      >
        <input
          type="checkbox"
          aria-label="开启均衡器"
          checked={equalizer.enabled}
          disabled={equalizer.unsupported}
          onchange={(e) => toggleEnabled(e.currentTarget)}
        />
        <span class="eq-switch-track" aria-hidden="true"></span>
      </label>

      <button class="eq-close" title="收起" aria-label="收起均衡器" onclick={closeEqualizer}>
        <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
      </button>
    </div>

    <div class="eq-body">
      <div class="eq-preset-head">
        <span>预设</span>
        <span class="eq-current">{currentPresetName()}</span>
      </div>
      <div class="eq-presets" role="group" aria-label="均衡器预设">
        {#each EQ_PRESETS as p (p.id)}
          <button
            class="eq-preset"
            class:on={equalizer.preset === p.id}
            title={p.desc}
            aria-pressed={equalizer.preset === p.id}
            onclick={() => setPreset(p.id)}
          >
            {p.name}
          </button>
        {/each}
      </div>

      <div class="eq-stage" class:off={!equalizer.enabled}>
        <div class="eq-bands">
          {#each EQ_BANDS as band, i (band.hz)}
            <EqBand label={band.label} value={equalizer.gains[i]} onchange={(db) => setBandGain(i, db)} />
          {/each}
        </div>

        <div class="eq-preamp">
          <span class="eq-preamp-label">前级</span>
          <input
            type="range"
            min={EQ_MIN_DB}
            max={EQ_MAX_DB}
            step="0.5"
            value={equalizer.preamp}
            style="--p:{preampPct}%"
            aria-label="前级增益"
            aria-valuetext={`${formatDb(equalizer.preamp)} dB`}
            oninput={(e) => setPreamp(Number(e.currentTarget.value))}
          />
          <span class="eq-preamp-val">{formatDb(equalizer.preamp)} dB</span>
        </div>
      </div>
    </div>

    <div class="eq-foot">
      <span class="eq-hint">
        {#if equalizer.unsupported}
          当前系统不支持音效处理，均衡器不可用
        {:else if equalizer.enabled}
          已开启 · 双击滑块可归零
        {:else}
          打开开关后生效
        {/if}
      </span>
      <button class="eq-reset" onclick={resetEqualizer} title="恢复平直曲线">重置</button>
    </div>
  </div>
{/if}

<style>
  .eq-mask {
    position: fixed;
    inset: 0;
    z-index: 77;
    background: transparent;
  }

  .eq-panel {
    position: fixed;
    right: 18px;
    bottom: 110px;
    z-index: 78;
    display: flex;
    flex-direction: column;
    width: min(438px, calc(100vw - 24px));
    max-height: calc(100vh - 132px);
    overflow: hidden;
    border-radius: var(--r-xl);
    background-color: var(--popover);
    background-clip: padding-box;
    box-shadow: var(--float-shadow);
    will-change: transform, opacity;
  }

  .eq-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 14px 10px 18px;
  }

  .eq-icon {
    flex: none;
    display: grid;
    place-items: center;
    color: var(--fg2);
  }

  .eq-heading {
    flex: 1;
    min-width: 0;
  }

  .eq-title {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 13.5px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }

  .eq-badge {
    padding: 1.5px 7px;
    border-radius: 999px;
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    color: var(--fg3);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0;
  }

  .eq-sub {
    margin-top: 2px;
    color: var(--fg3);
    font-size: 10.5px;
    line-height: 1.3;
  }

  .eq-switch {
    position: relative;
    flex: none;
    display: inline-flex;
    width: 38px;
    height: 22px;
    cursor: pointer;
  }

  .eq-switch input {
    position: absolute;
    inset: 0;
    z-index: 1;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .eq-switch-track {
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background-color: var(--track);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    transition: background-color 160ms ease;
  }

  .eq-switch-track::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background-color: var(--knob);
    box-shadow: 0 1px 3px oklch(0 0 0 / 28%);
    transition: transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .eq-switch input:checked + .eq-switch-track {
    background-color: #0a84ff;
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 8%);
  }

  .eq-switch input:checked + .eq-switch-track::after {
    transform: translateX(16px);
  }

  .eq-switch input:focus-visible + .eq-switch-track {
    outline: 2px solid var(--focus-line);
    outline-offset: 2px;
  }

  .eq-switch.disabled {
    opacity: 0.4;
    cursor: default;
  }

  .eq-switch.disabled input {
    cursor: default;
  }

  .eq-close {
    width: 28px;
    height: 28px;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--r-sm);
    color: var(--fg3);
  }

  .eq-close:hover {
    background-color: var(--glass-state);
    box-shadow: var(--state-shadow);
    color: var(--fg);
  }

  .eq-body {
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 2px;
    scrollbar-gutter: stabilize;
  }

  .eq-preset-head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 0 18px 6px;
    color: var(--fg3);
    font-size: 10.5px;
  }

  .eq-current {
    margin-left: auto;
    color: var(--fg2);
    font-size: 10.5px;
  }

  .eq-presets {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 6px;
    padding: 0 18px 14px;
  }

  .eq-preset {
    height: 27px;
    min-width: 0;
    padding: 0 4px;
    overflow: hidden;
    border-radius: var(--r-sm);
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    color: var(--fg2);
    font-size: 11.5px;
    line-height: 27px;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition:
      background-color 140ms ease-out,
      color 140ms ease-out,
      box-shadow 140ms ease-out;
  }

  .eq-preset:hover:not(.on) {
    background-color: color-mix(in oklch, var(--fg) 12%, transparent);
    color: var(--fg);
  }

  .eq-preset.on {
    background-color: var(--glass-state);
    box-shadow: var(--state-shadow);
    color: var(--fg);
    font-weight: 600;
  }

  .eq-stage {
    transition: opacity 180ms ease-out;
  }

  .eq-stage.off {
    opacity: 0.55;
  }

  .eq-bands {
    position: relative;
    display: flex;
    gap: 2px;
    height: 184px;
    padding: 0 14px;
  }

  /* 0 dB 参考线：从频段之间的空隙透出来，滑块本体盖住中间几像素 */
  .eq-bands::before {
    content: '';
    position: absolute;
    left: 14px;
    right: 14px;
    top: 50%;
    height: 1px;
    background-color: var(--line);
    pointer-events: none;
  }

  .eq-preamp {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 18px 10px;
  }

  .eq-preamp-label {
    flex: none;
    color: var(--fg3);
    font-size: 10.5px;
  }

  .eq-preamp input[type='range'] {
    flex: 1;
    min-width: 0;
  }

  .eq-preamp-val {
    flex: none;
    width: 52px;
    color: var(--fg3);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .eq-foot {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 18px 11px;
    border-top: 1px solid var(--line);
  }

  .eq-hint {
    flex: 1;
    min-width: 0;
    color: var(--fg3);
    font-size: 10.5px;
    line-height: 1.4;
  }

  .eq-reset {
    flex: none;
    height: 26px;
    padding: 0 11px;
    border-radius: var(--r-sm);
    background-color: var(--glass-control);
    box-shadow: var(--control-shadow);
    color: var(--fg2);
    font-size: 11.5px;
    white-space: nowrap;
  }

  .eq-reset:hover {
    background-color: color-mix(in oklch, var(--fg) 12%, transparent);
    color: var(--fg);
  }

  @media (max-height: 620px) {
    .eq-panel {
      bottom: 92px;
      max-height: calc(100vh - 108px);
    }

    .eq-bands {
      height: 150px;
    }
  }
</style>
