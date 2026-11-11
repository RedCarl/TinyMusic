<script lang="ts">
  import { HugeiconsIcon, type IconSvgElement } from '@hugeicons/svelte';

  let {
    icon,
    size = 24
  }: {
    icon: IconSvgElement;
    size?: number | string;
  } = $props();
</script>

<!--
  @hugeicons/svelte 1.1.5 只在 onMount 时把图标画一次：同一个实例换 `icon` 属性
  不会重绘（见 HugeiconsIcon.svelte 的 propsForUpdate，里面不含 icon）。
  播放/暂停、加载中这类会切换的图标必须让实例重建，否则按钮一直停在第一次渲染的图标上。
  静态图标直接用 HugeiconsIcon 即可，这里只是给「会变的图标」准备的。
-->
{#key icon}
  <HugeiconsIcon {icon} {size} />
{/key}
