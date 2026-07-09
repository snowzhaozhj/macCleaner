<script lang="ts">
  /**
   * 稳定三区外壳原语（U1 / R1 / KTD1）。
   *
   * route 内用 CSS grid 固定三区：摘要 / 列表 / 操作。三个区块的根节点由本组件恒定持有，
   * phase 只替换各 snippet 传入的内容——**永不用 {#if} 增删整块区域**，从而相位切换
   * （idle→scanning→results→done）时页面高度与分区数不变、无区块 mount/unmount（防跳变基座）。
   *
   * 列表区 min-height:0 + overflow 使其成为唯一可滚动/伸缩区；摘要与操作区高度随内容，
   * 但节点持续存在（内容为空时塌陷为 0 高，不销毁节点）。
   */
  import type { Snippet } from "svelte";

  let {
    summary,
    list,
    actions,
  }: {
    summary: Snippet;
    list: Snippet;
    actions: Snippet;
  } = $props();
</script>

<div class="shell-route">
  <div class="slot slot-summary">{@render summary()}</div>
  <div class="slot slot-list">{@render list()}</div>
  <div class="slot slot-actions">{@render actions()}</div>
</div>

<style>
  .shell-route {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    height: 100%;
    min-height: 0;
    gap: var(--sp-3);
  }
  .slot {
    min-width: 0;
  }
  /* 列表区是唯一伸缩+滚动区；摘要/操作区随内容，但节点恒在 */
  .slot-list {
    min-height: 0;
    overflow-y: auto;
  }
</style>
