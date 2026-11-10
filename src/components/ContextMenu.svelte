<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { ICON_SIZE } from '../lib/icons';
  import { ctxMenu, closeMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { showToast } from '../lib/toast.svelte';

  let el = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 0, y: 0 });

  /** 当前展开的二级菜单（只支持一层，音质选择够用） */
  let subOpen = $state<number | null>(null);
  let subEl = $state<HTMLDivElement | null>(null);
  let subAnchor = $state({ left: 0, right: 0, top: 0, bottom: 0 });
  let subPos = $state({ x: 0, y: 0 });
  let subReady = $state(false);
  let closeTimer: ReturnType<typeof setTimeout> | undefined;

  // 先渲染再量尺寸，避免菜单超出窗口右侧/底部
  $effect(() => {
    if (!ctxMenu.open) return;
    void ctxMenu.items.length;
    requestAnimationFrame(() => {
      const w = el?.offsetWidth ?? 220;
      const h = el?.offsetHeight ?? 40;
      pos = {
        x: Math.max(8, Math.min(ctxMenu.x, window.innerWidth - w - 8)),
        y: Math.max(8, Math.min(ctxMenu.y, window.innerHeight - h - 8)),
      };
    });
  });

  async function openSub(i: number, e: MouseEvent) {
    clearTimeout(closeTimer);
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    subAnchor = { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom };
    subPos = { x: rect.right + 2, y: rect.top - 6 };
    subReady = false;
    subOpen = i;
    await tick();
    measureSub();
  }

  function scheduleCloseSub() {
    clearTimeout(closeTimer);
    closeTimer = setTimeout(() => {
      subOpen = null;
      subReady = false;
    }, 120);
  }

  function cancelCloseSub() {
    clearTimeout(closeTimer);
  }

  // 二级菜单渲染后量一次尺寸，做左右翻转和上下夹取，保证不超出窗口。
  function measureSub() {
    if (!subEl) return;
    const w = subEl.offsetWidth || 196;
    const h = subEl.offsetHeight || 240;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let x = subAnchor.right + 2;
    if (x + w > vw - 8) x = subAnchor.left - w - 2;
    x = Math.max(8, Math.min(x, vw - w - 8));
    let y = subAnchor.top - 6;
    if (y + h > vh - 8) y = vh - h - 8;
    y = Math.max(8, y);
    subPos = { x, y };
    subReady = true;
  }

  async function run(item: MenuItem) {
    if (!item.run) return;
    closeMenu();
    try {
      await item.run();
    } catch (e) {
      showToast(String(e));
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') closeMenu();
  }

  const onScroll = () => closeMenu();
  const onBlur = () => closeMenu();

  $effect(() => {
    if (!ctxMenu.open) return;
    subOpen = null;
    window.addEventListener('scroll', onScroll, true);
    window.addEventListener('blur', onBlur);
    window.addEventListener('resize', onScroll);
    return () => {
      window.removeEventListener('scroll', onScroll, true);
      window.removeEventListener('blur', onBlur);
      window.removeEventListener('resize', onScroll);
    };
  });

  onDestroy(() => closeMenu());
</script>

<svelte:window onkeydown={onKey} />

{#if ctxMenu.open}
  <div
    class="ctx-backdrop"
    role="presentation"
    onclick={closeMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeMenu();
    }}
  ></div>
  <div
    class="ctx-menu"
    bind:this={el}
    role="menu"
    tabindex="-1"
    aria-label="操作菜单"
    style="left:{pos.x}px; top:{pos.y}px"
    oncontextmenu={(e) => e.preventDefault()}
  >
    {#each ctxMenu.items as item, i (i)}
      {#if 'sep' in item}
        <div class="ctx-sep" role="separator"></div>
      {:else if item.submenu}
        <div
          class="ctx-sub-wrap"
          role="presentation"
          onmouseenter={(e) => openSub(i, e)}
          onmouseleave={scheduleCloseSub}
        >
          <button
            class="ctx-item"
            class:danger={item.danger}
            class:selected={item.selected}
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={subOpen === i}
            disabled={item.disabled}
            onclick={() => (subOpen = subOpen === i ? null : i)}
          >
            <span class="ctx-ico">
              {#if item.icon}
                <HugeiconsIcon icon={item.icon} size={ICON_SIZE.inline} />
              {/if}
            </span>
            <span class="ctx-label">{item.label}</span>
            <span class="ctx-arrow" aria-hidden="true">›</span>
          </button>

          {#if subOpen === i}
            <div
              class="ctx-menu ctx-submenu"
              class:ready={subReady}
              bind:this={subEl}
              role="menu"
              tabindex="-1"
              aria-label={item.label}
              style="left:{subPos.x}px; top:{subPos.y}px"
              onmouseenter={cancelCloseSub}
              onmouseleave={scheduleCloseSub}
            >
              {#each item.submenu as sub, j (j)}
                {#if 'sep' in sub}
                  <div class="ctx-sep" role="separator"></div>
                {:else}
                  <button
                    class="ctx-item"
                    class:danger={sub.danger}
                    class:selected={sub.selected}
                    role="menuitem"
                    disabled={sub.disabled}
                    onclick={() => run(sub)}
                  >
                    <span class="ctx-ico">
                      {#if sub.icon}
                        <HugeiconsIcon icon={sub.icon} size={ICON_SIZE.inline} />
                      {/if}
                    </span>
                    <span class="ctx-label">{sub.label}</span>
                    {#if sub.hint}<span class="ctx-hint">{sub.hint}</span>{/if}
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        </div>
      {:else}
        <button
          class="ctx-item"
          class:danger={item.danger}
          class:selected={item.selected}
          role="menuitem"
          disabled={item.disabled}
          onclick={() => run(item)}
        >
          <span class="ctx-ico">
            {#if item.icon}
              <HugeiconsIcon icon={item.icon} size={ICON_SIZE.inline} />
            {/if}
          </span>
          <span class="ctx-label">{item.label}</span>
          {#if item.hint}<span class="ctx-hint">{item.hint}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
{/if}
