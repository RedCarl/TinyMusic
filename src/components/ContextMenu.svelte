<script lang="ts">
  import { onDestroy } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { ICON_SIZE } from '../lib/icons';
  import { ctxMenu, closeMenu, type MenuItem } from '../lib/contextmenu.svelte';
  import { showToast } from '../lib/toast.svelte';

  let el = $state<HTMLDivElement | null>(null);
  let pos = $state({ x: 0, y: 0 });

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

  async function run(item: MenuItem) {
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
      {:else}
        <button
          class="ctx-item"
          class:danger={item.danger}
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
