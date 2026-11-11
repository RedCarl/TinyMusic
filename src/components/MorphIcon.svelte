<script lang="ts">
  import { MorphIcon as MorphingIcon, type IconInput } from 'morphicons/svelte';
  import type { IconSvgElement } from '@hugeicons/svelte';

  /**
   * 会切换的图标走这一层：底层是 morphicons，`icon` 一换就用弹簧动画把旧形状
   * 扯成新形状（播放↔暂停、音量三档、循环↔单曲循环）。
   * 静态图标继续直接用 HugeiconsIcon，不用套这层。
   */
  let {
    icon,
    size = 24,
    strokeWidth = 1.5,
    label
  }: {
    icon: IconSvgElement;
    size?: number | string;
    strokeWidth?: number;
    label?: string;
  } = $props();
</script>

<MorphingIcon
  icon={icon as unknown as IconInput}
  {size}
  {strokeWidth}
  {label}
  spring="snappy"
  reducedMotion="user"
/>
