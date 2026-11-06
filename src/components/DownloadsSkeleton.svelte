<script lang="ts">
  /** 下载页：summary 是真实可见的，这里只对齐下面的 6 列表格 */
  let { rows = 6 }: { rows?: number } = $props();
</script>

<div class="sk-block" role="status" aria-label="加载中">
  <table class="dl sk-dl" aria-hidden="true">
    <thead>
      <tr>
        <th>歌曲</th>
        <th>艺人</th>
        <th>音质</th>
        <th>位置</th>
        <th>大小</th>
        <th>状态</th>
      </tr>
    </thead>
    <tbody>
      {#each Array(rows) as _, i (i)}
        <tr class="sk-tr">
          <td><span class="sk sk-cell" style="width:72%"></span></td>
          <td><span class="sk sk-cell" style="width:56%"></span></td>
          <td><span class="sk sk-cell" style="width:34px"></span></td>
          <td><span class="sk sk-cell" style="width:40px"></span></td>
          <td><span class="sk sk-cell" style="width:48px"></span></td>
          <td><span class="sk sk-cell" style="width:52px"></span></td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>
