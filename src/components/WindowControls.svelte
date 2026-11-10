<script lang="ts">
  import { onMount } from 'svelte';
  import { getCurrentWindow } from '@tauri-apps/api/window';

  /**
   * 窗口按钮（Windows 自绘标题栏的一部分）。
   *
   * 固定在右上角而不是跟着标题栏走：歌词页是盖满整个窗口的浮层，
   * 弹窗也有自己的遮罩 —— 按钮浮在最上面，任何界面都还能最小化/关闭。
   */
  const hasWindow = '__TAURI_INTERNALS__' in window;
  const win = hasWindow ? getCurrentWindow() : null;

  let maximized = $state(false);

  async function refresh() {
    if (!win) return;
    maximized = await win.isMaximized().catch(() => false);
  }

  /** 拖动缩放时 resize 事件很密，攒一下再问窗口状态 */
  let timer: ReturnType<typeof setTimeout> | undefined;
  function refreshSoon() {
    clearTimeout(timer);
    timer = setTimeout(() => void refresh(), 120);
  }

  onMount(() => {
    void refresh();
    // 双击拖动条、Win+↑、贴边吸附都会改最大化状态：跟着窗口尺寸同步按钮图标
    let disposed = false;
    let unlisten: (() => void) | undefined;
    win?.onResized(() => refreshSoon())
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    // 从托盘恢复、别的窗口里最大化之后再切回来，尺寸事件不一定补发
    const onFocus = () => void refresh();
    window.addEventListener('focus', onFocus);
    return () => {
      disposed = true;
      clearTimeout(timer);
      unlisten?.();
      window.removeEventListener('focus', onFocus);
    };
  });

  function minimize() {
    win?.minimize().catch(() => {});
  }

  async function toggle() {
    if (!win) return;
    await win.toggleMaximize().catch(() => {});
    void refresh();
  }

  function close() {
    win?.close().catch(() => {});
  }
</script>

{#if hasWindow}
  <div class="win-controls" role="group" aria-label="窗口控制">
    <button class="wc" onclick={minimize} title="最小化" aria-label="最小化">
      <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
        <path d="M1 6h10" />
      </svg>
    </button>

    <button
      class="wc"
      onclick={toggle}
      title={maximized ? '向下还原' : '最大化'}
      aria-label={maximized ? '向下还原' : '最大化'}
      aria-pressed={maximized}
    >
      <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
        {#if maximized}
          <rect x="4" y="1.5" width="6.5" height="6.5" rx="1" />
          <path d="M3.5 3.5H2.5A1 1 0 0 0 1.5 4.5v4a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1" />
        {:else}
          <rect x="1.5" y="1.5" width="9" height="9" rx="1.2" />
        {/if}
      </svg>
    </button>

    <button class="wc close" onclick={close} title="关闭" aria-label="关闭">
      <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
        <path d="M1.5 1.5 10.5 10.5M10.5 1.5 1.5 10.5" />
      </svg>
    </button>
  </div>
{/if}
