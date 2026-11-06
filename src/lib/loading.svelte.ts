import { untrack } from 'svelte';

export interface DelayedVisibleOptions {
  /** 加载持续多久之后才显示骨架屏，避免快页面闪一下 */
  delay?: number;
  /** 一旦显示，至少保留多久，避免骨架屏一闪就被内容替换 */
  minShow?: number;
}

/**
 * 把「原始 loading」转换成「是否显示骨架屏」。
 *
 * - loading 在 delay 内结束：一次都不显示骨架屏
 * - loading 超过 delay：显示骨架屏
 * - 显示后即使 loading 已经结束，也至少再保留 minShow 毫秒，
 *   让用户看清骨架结构，而不是看它闪一帧
 *
 * 用法（在组件顶层调用）：
 *   const skel = delayedVisible(() => loading);
 *   {#if skel.visible} <SomeSkeleton /> {:else if !loading} 内容 {/if}
 */
export function delayedVisible(
  loading: () => boolean,
  { delay = 160, minShow = 320 }: DelayedVisibleOptions = {}
) {
  let visible = $state(false);
  let shownAt = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    const active = loading();
    clearTimeout(timer);
    const shown = untrack(() => visible);

    if (active) {
      if (shown) {
        // 上一轮骨架还在展示时又开始新一轮加载：重新计算最短展示时间
        shownAt = performance.now();
      } else {
        timer = setTimeout(() => {
          visible = true;
          shownAt = performance.now();
        }, delay);
      }
    } else if (shown) {
      const left = minShow - (performance.now() - shownAt);
      if (left > 0) {
        timer = setTimeout(() => {
          visible = false;
        }, left);
      } else {
        visible = false;
      }
    }

    return () => clearTimeout(timer);
  });

  return {
    get visible() {
      return visible;
    }
  };
}
