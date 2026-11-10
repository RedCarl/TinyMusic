<script lang="ts">
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { icons, ICON_SIZE } from '../lib/icons';
</script>

<!--
  Windows 自绘标题栏。

  整条都是窗口的「把手」：`data-tauri-drag-region="deep"` 让品牌名、空白处
  都能拖动窗口，双击切换最大化/还原（对齐系统标题栏的手感）。
  右上角的按钮是固定定位的 WindowControls，不放在这一层里 ——
  歌词页、弹窗浮上来之后它们仍然在，窗口随时能关。
-->
<header class="titlebar" data-tauri-drag-region="deep">
  <div class="brand">
    <span class="logo"><HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} /></span>
    <span>TinyMusic</span>
  </div>
</header>
