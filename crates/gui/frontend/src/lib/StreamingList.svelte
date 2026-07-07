<script lang="ts">
  /**
   * 防跳变流式列表原语（U2 / R2 R4 R5 / KTD2）。
   *
   * 行粒度 = **分类**（clean_rules 已知类预印为占位行；逐项明细是展开的第二层 F2）。
   * 之所以以分类为行：个体路径无法在发现前预知，若逐项 append 会产生可见的行插入跳变——
   * 正是本次重设计要消灭的。故扫描期只呈现固定的分类行（骨架→原位填数字），
   * 「逐帧无行新增/移除」由此成立（Success Criteria，录屏可验）。
   *
   * 四件套：
   *  1) keyed 行（key=分类名，稳定）＋预印分类头（knownOrder）。
   *  2) rAF 批处理——由父组件把 Found 入缓冲、每帧 flush 一次（本组件只消费聚合结果，
   *     聚合是 $derived，Svelte 自身按帧调度渲染）。
   *  3) 骨架——0 命中分类在扫描期显示骨架条。
   *  4) 完成时一次 FLIP settle——results 相位切到体积降序 + 收拢空分类，`animate:flip`
   *     只在此刻触发一次重排动画（≤--dur-settle）；扫描期顺序=knownOrder 恒定，绝不重排。
   */
  import { flip } from "svelte/animate";
  import { aggregateByCategory, formatBytes, type LiveItem } from "./format";
  import Safety from "./Safety.svelte";
  import EvidenceCard from "./EvidenceCard.svelte";
  import PathText from "./PathText.svelte";

  let {
    items,
    knownOrder = [],
    scanning,
    onToggle,
  }: {
    items: LiveItem[];
    knownOrder?: readonly string[];
    scanning: boolean;
    /** 逐项勾选（仅 results 相位可用；扫描期列表不可交互）。 */
    onToggle?: (item: LiveItem) => void;
  } = $props();

  // 扫描期：保留 0 命中已知分类（骨架），顺序=knownOrder 锁死不重排（R2/R4）。
  // 完成期：收拢空分类（R3）+ 体积降序（R4 的 settle 目标序，交给 flip 动一次）。
  const cats = $derived.by(() => {
    const aggs = aggregateByCategory(items, knownOrder, !scanning);
    if (scanning) return aggs;
    return [...aggs].sort((a, b) => b.size - a.size);
  });

  // 展开态（results 第二层）：默认全折叠，用户点分类头展开审查逐项。
  let expanded = $state<Set<string>>(new Set());
  function toggleExpand(name: string) {
    if (scanning) return; // 扫描期禁止展开（列表在实时填充）
    const next = new Set(expanded);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    expanded = next;
  }
</script>

<ul class="cats">
  {#each cats as cat (cat.name)}
    <li class="cat" animate:flip={{ duration: 250 }}>
      <button
        class="cat-head"
        class:interactive={!scanning}
        aria-expanded={expanded.has(cat.name)}
        disabled={scanning || cat.count === 0}
        onclick={() => toggleExpand(cat.name)}
      >
        <span class="chevron" class:open={expanded.has(cat.name)} aria-hidden="true">
          <svg viewBox="0 0 10 10" width="10" height="10">
            <path d="M3 2 L7 5 L3 8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
          </svg>
        </span>
        <span class="cat-name">{cat.name}</span>
        {#if cat.count === 0 && scanning}
          <span class="skeleton" aria-label="扫描中"></span>
        {:else}
          <span class="cat-meta">
            <span class="count">{cat.count} 项</span>
            <span class="cat-size">{formatBytes(cat.size)}</span>
          </span>
        {/if}
      </button>

      {#if expanded.has(cat.name) && !scanning}
        <ul class="rows">
          {#each cat.items as item (item.path)}
            <li class="row" class:selected={item.selected}>
              <label class="check">
                <input
                  type="checkbox"
                  checked={item.selected}
                  onchange={() => onToggle?.(item)}
                  aria-label={item.path}
                />
              </label>
              <Safety safety={item.safety} />
              <PathText path={item.path} />
              <EvidenceCard impact={item.impact} recovery={item.recovery} />
              <span class="size">{formatBytes(item.size)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .cats {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .cat {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-raised);
    overflow: hidden;
  }
  .cat-head {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-3);
    background: none;
    border: none;
    color: var(--ink-primary);
    font-family: var(--font-ui);
    font-size: 0.95em;
    text-align: left;
    cursor: default;
  }
  .cat-head.interactive:not(:disabled) {
    cursor: pointer;
  }
  .cat-head.interactive:not(:disabled):hover {
    background: var(--surface-overlay);
  }
  .chevron {
    flex: 0 0 auto;
    display: inline-flex;
    color: var(--ink-faint);
    transition: transform var(--dur-fast) var(--ease-out-quart);
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  /* 扫描期禁用态：隐藏 chevron，分类行只读 */
  .cat-head:disabled .chevron {
    visibility: hidden;
  }
  .cat-name {
    flex: 1 1 auto;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cat-meta {
    flex: 0 0 auto;
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
  }
  .count {
    color: var(--ink-muted);
    font-variant-numeric: tabular-nums;
  }
  .cat-size {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-primary);
  }
  .skeleton {
    flex: 0 0 auto;
    width: 72px;
    height: 0.9em;
    border-radius: 4px;
    background: linear-gradient(
      90deg,
      var(--surface-overlay) 25%,
      var(--border-subtle) 50%,
      var(--surface-overlay) 75%
    );
    background-size: 200% 100%;
    animation: shimmer 1.2s linear infinite;
  }
  @keyframes shimmer {
    to {
      background-position: -200% 0;
    }
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0 var(--sp-2) var(--sp-2);
    border-top: 1px solid var(--border-subtle);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: var(--row-height);
    padding: 0 var(--sp-1);
  }
  .row:hover {
    background: var(--surface-overlay);
  }
  .row.selected {
    background: color-mix(in oklch, var(--accent) 10%, transparent);
  }
  .check {
    display: flex;
    flex: 0 0 auto;
  }
  .check input {
    accent-color: var(--accent);
    width: 15px;
    height: 15px;
    cursor: pointer;
  }
  .size {
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-muted);
  }
</style>
