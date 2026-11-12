<script lang="ts">
  import { EQ_MAX_DB, EQ_MIN_DB, formatDb } from '../lib/equalizer.svelte';

  interface Props {
    label: string;
    value: number;
    onchange: (db: number) => void;
  }

  let { label, value, onchange }: Props = $props();

  let trackEl = $state<HTMLElement | null>(null);
  let dragging = $state(false);

  const thumbPct = $derived(((value - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB)) * 100);
  const fillBottom = $derived(Math.min(thumbPct, 50));
  const fillHeight = $derived(Math.abs(thumbPct - 50));

  function clamp(v: number): number {
    return Math.max(EQ_MIN_DB, Math.min(EQ_MAX_DB, v));
  }

  function valueFromPointer(e: PointerEvent): number {
    const el = trackEl;
    if (!el) return value;
    const rect = el.getBoundingClientRect();
    if (rect.height <= 0) return value;
    const y = Math.min(rect.bottom, Math.max(rect.top, e.clientY));
    const ratio = 1 - (y - rect.top) / rect.height;
    return clamp(Math.round((EQ_MIN_DB + ratio * (EQ_MAX_DB - EQ_MIN_DB)) * 2) / 2);
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    onchange(valueFromPointer(e));
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    onchange(valueFromPointer(e));
  }

  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture?.(e.pointerId);
  }

  function onKeyDown(e: KeyboardEvent) {
    const step = e.shiftKey ? 1 : 0.5;
    if (e.key === 'ArrowUp' || e.key === 'ArrowRight') {
      e.preventDefault();
      onchange(clamp(value + step));
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') {
      e.preventDefault();
      onchange(clamp(value - step));
    } else if (e.key === 'PageUp') {
      e.preventDefault();
      onchange(clamp(value + 3));
    } else if (e.key === 'PageDown') {
      e.preventDefault();
      onchange(clamp(value - 3));
    } else if (e.key === 'Home') {
      e.preventDefault();
      onchange(EQ_MIN_DB);
    } else if (e.key === 'End') {
      e.preventDefault();
      onchange(EQ_MAX_DB);
    }
  }

  function onDoubleClick() {
    onchange(0);
  }
</script>

<div class="band">
  <span class="db" class:zero={value === 0}>{formatDb(value)}</span>
  <div
    class="track"
    class:dragging
    bind:this={trackEl}
    role="slider"
    tabindex="0"
    aria-label={`${label} 赫兹`}
    aria-orientation="vertical"
    aria-valuemin={EQ_MIN_DB}
    aria-valuemax={EQ_MAX_DB}
    aria-valuenow={value}
    aria-valuetext={`${formatDb(value)} dB`}
    title={`${label} Hz：${formatDb(value)} dB（双击归零）`}
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    onkeydown={onKeyDown}
    ondblclick={onDoubleClick}
  >
    <span class="rail" aria-hidden="true"></span>
    <span
      class="fill"
      aria-hidden="true"
      style="bottom:{fillBottom}%; height:{fillHeight}%"
    ></span>
    <span class="thumb" aria-hidden="true" style="bottom:{thumbPct}%"></span>
  </div>
  <span class="hz">{label}</span>
</div>

<style>
  .band {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }

  .db {
    color: var(--fg3);
    font-size: 10px;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }

  .db.zero {
    color: var(--fg3);
    opacity: 0.55;
  }

  .track {
    position: relative;
    width: 100%;
    max-width: 28px;
    flex: 1;
    margin: 7px 0;
    border-radius: 6px;
    cursor: ns-resize;
    touch-action: none;
    outline: none;
  }

  .track:focus-visible {
    outline: 2px solid var(--focus-line);
    outline-offset: 2px;
  }

  .rail {
    position: absolute;
    left: 50%;
    top: 0;
    bottom: 0;
    width: 4px;
    margin-left: -2px;
    border-radius: 999px;
    background-color: var(--track);
    box-shadow: var(--well-shadow);
  }

  .fill {
    position: absolute;
    left: 50%;
    width: 4px;
    margin-left: -2px;
    border-radius: 999px;
    background-color: var(--ink);
    pointer-events: none;
    transition:
      bottom 60ms linear,
      height 60ms linear;
  }

  .track.dragging .fill {
    transition: none;
  }

  .thumb {
    position: absolute;
    left: 50%;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background-color: var(--knob);
    box-shadow: var(--knob-shadow);
    transform: translate(-50%, 50%);
    pointer-events: none;
    transition: bottom 60ms linear;
  }

  .track.dragging .thumb {
    transition: none;
  }

  .hz {
    color: var(--fg3);
    font-size: 9.5px;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.01em;
  }
</style>
