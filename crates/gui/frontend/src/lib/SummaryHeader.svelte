<script lang="ts">
  /**
   * 首屏摘要（U4 / R7 R8 R10 / R17 R18）。
   *
   * 一句话答案「可安全释放 X」——主数字来自当前已选总和（v1 Clean 全 Safe＝全部），
   * 与操作区主按钮量恒等（R10）。扫描累加时数字随 $derived 增长；完成后 UI 不做 count-up
   * 动画（R19：仅绑定值，不补间），故「定格无 count-up」天然成立。
   *
   * macOS 储存空间式静态分段横条：低饱和分类色 + 图例带精确值；Safe 段绝不用红系（R18）。
   * 数字用等宽 tabular-nums，强调靠 weight 而非 display 尺寸（R17 字阶 ≤3 级）。
   */
  import { formatBytes, type Segment } from "./format";

  let {
    selectedSize,
    segments,
    scanning = false,
  }: {
    selectedSize: number;
    segments: Segment[];
    scanning?: boolean;
  } = $props();

  // 低饱和分段色（按发现序索引取；刻意避开红系——安全内容不恐吓，R18）。
  const PALETTE = [
    "oklch(0.68 0.09 210)", // 冷蓝：系统缓存
    "oklch(0.72 0.08 160)", // 低饱和绿：浏览器缓存
    "oklch(0.7 0.07 260)", // 靛
    "oklch(0.74 0.07 90)", // 暖黄（低饱和，非警示红）
  ];
  function colorAt(i: number): string {
    return PALETTE[i % PALETTE.length];
  }
</script>

<header class="summary">
  <p class="headline">
    <span class="lead">可安全释放</span>
    <span class="amount">{formatBytes(selectedSize)}</span>
    {#if scanning}<span class="scanning-tag">扫描中…</span>{/if}
  </p>

  <div class="bar" aria-hidden="true">
    {#if segments.length === 0}
      <span class="bar-empty"></span>
    {:else}
      {#each segments as seg, i (seg.name)}
        <span
          class="seg"
          style="width: {seg.fraction * 100}%; background: {colorAt(i)}"
        ></span>
      {/each}
    {/if}
  </div>

  {#if segments.length > 0}
    <ul class="legend">
      {#each segments as seg, i (seg.name)}
        <li>
          <span class="dot" style="background: {colorAt(i)}"></span>
          <span class="legend-name">{seg.name}</span>
          <span class="legend-size">{formatBytes(seg.size)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</header>

<style>
  .summary {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .headline {
    margin: 0;
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    flex-wrap: wrap;
  }
  .lead {
    font-family: var(--font-ui);
    font-size: 1rem;
    color: var(--ink-muted);
  }
  /* 主数字：weight 强调，非 display 尺寸（R17）；等宽 tabular-nums 防累加抖动 */
  .amount {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-size: 1.7rem;
    font-weight: 700;
    color: var(--ink-primary);
    line-height: 1.1;
  }
  .scanning-tag {
    font-size: 0.85em;
    color: var(--ink-muted);
  }
  .bar {
    display: flex;
    height: 12px;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface-raised);
  }
  .seg {
    height: 100%;
    /*
     * 不对 width 做过渡：扫描期占比每帧更新，200ms 过渡会不断重启、永远追不上真值
     * 而显得滞后；逐帧快照本身就是平滑增长，且诚实反映当前累加值（R19 动效只传达状态）。
     */
  }
  .seg + .seg {
    box-shadow: inset 1px 0 0 var(--surface-base);
  }
  .bar-empty {
    width: 100%;
    height: 100%;
    background: var(--surface-raised);
  }
  .legend {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-4);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: 0.85em;
  }
  .dot {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    flex: 0 0 auto;
  }
  .legend-name {
    color: var(--ink-muted);
  }
  .legend-size {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-primary);
  }
</style>
