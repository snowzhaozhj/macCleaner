<script lang="ts">
  /**
   * 防跳变流式列表原语（U2 / R2 R4 R5 / KTD2）＋ move 6 渐进披露·展开=换问题。
   *
   * 行粒度 = **分类**（clean_rules 已知类预印为占位行；逐项明细是展开的第二层 F2）。
   * 之所以以分类为行：个体路径无法在发现前预知，若逐项 append 会产生可见的行插入跳变——
   * 正是本次重设计要消灭的。故扫描期只呈现固定的分类行（骨架→原位填数字），
   * 「逐帧无行新增/移除」由此成立（Success Criteria，录屏可验）。
   *
   * 防跳变四件套：keyed 行 + rAF 批处理（父组件）+ 骨架 + 完成时一次 FLIP settle。
   *
   * move 6「恰好两层」：
   *  - **折叠层（分类行）**答普通用户「值不值删」——分类名 + 体积占比条 + count/size（≤4 元素）。
   *  - **展开层（审查面孔）**换一副面孔答开发者「它到底是什么」——完整路径可复制 +
   *    impact/recovery 全文 + 「在 Finder 中显示」；顶部给一行可复制的现存等价命令
   *    `mc clean`（把 GUI 用户体面送回终端的出口）。逐项精确 `--only` 命令因 CLI 未支持而
   *    诚实延后（不假造），见计划 018 路 B。
   */
  import { flip } from "svelte/animate";
  import { aggregateByCategory, formatBytes, type LiveItem } from "./format";
  import { revealInFinder } from "./ipc";
  import Safety from "./Safety.svelte";
  import EvidenceCard from "./EvidenceCard.svelte";
  import CopyButton from "./CopyButton.svelte";

  let {
    items,
    knownOrder = [],
    scanning,
    onToggle,
    cliCommand = "mc clean",
    cliNote = "清理全部可安全释放项",
  }: {
    items: LiveItem[];
    knownOrder?: readonly string[];
    scanning: boolean;
    /** 逐项勾选（仅 results 相位可用；扫描期列表不可交互）。 */
    onToggle?: (item: LiveItem) => void;
    /** 命令行等价出口的命令文本（Clean 默认 `mc clean`；Purge 传 `mc purge <目录>`）。 */
    cliCommand?: string;
    /** 命令行等价出口的诚实标注文案。 */
    cliNote?: string;
  } = $props();

  // 扫描期：保留 0 命中已知分类（骨架），顺序=knownOrder 锁死不重排（R2/R4）。
  // 完成期：收拢空分类（R3）+ 体积降序（R4 的 settle 目标序，交给 flip 动一次）。
  const cats = $derived.by(() => {
    const aggs = aggregateByCategory(items, knownOrder, !scanning);
    if (scanning) return aggs;
    return [...aggs].sort((a, b) => b.size - a.size);
  });

  const hasItems = $derived(cats.some((c) => c.count > 0));
  // 占比条口径 = 各分类占总命中体积之比（与分类行显示的 size 数字同源，读数一致）。
  const totalSize = $derived(cats.reduce((s, c) => s + c.size, 0));
  function fractionOf(size: number): number {
    return totalSize > 0 ? size / totalSize : 0;
  }

  // 命令行等价出口（move 6 / 路 B）：命令文本由 props 注入（Clean=`mc clean`、Purge=`mc purge <目录>`）。
  // 逐项精确 `--only <slug>` 需 CLI 支持，未支持前不展示不精确/不存在的命令（诚实招牌）。
  // 展开态（results 第二层）：默认全折叠，用户点分类头展开审查逐项。
  let expanded = $state<Set<string>>(new Set());
  function toggleExpand(name: string) {
    if (scanning) return; // 扫描期禁止展开（列表在实时填充）
    const next = new Set(expanded);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    expanded = next;
  }

  // 「在 Finder 中显示」失败不静默（路径可能刚被删/移）——落到列表底部横幅。
  let actionError = $state<string | null>(null);
  async function reveal(path: string) {
    actionError = null;
    try {
      await revealInFinder(path);
    } catch (e) {
      actionError = `在 Finder 中显示失败：${String(e)}`;
    }
  }
</script>

{#if hasItems && !scanning}
  <!-- 命令行等价出口：一次呈现，代表「用命令行清这一批」；诚实标注清理全部 -->
  <div class="cli-hint">
    <span class="cli-label">命令行等价</span>
    <code class="cli-cmd">{cliCommand}</code>
    <CopyButton text={cliCommand} />
    <span class="cli-note">{cliNote}</span>
  </div>
{/if}

<ul class="cats">
  {#each cats as cat (cat.name)}
    <!--
      flip 仅用于「完成时一次体积降序 settle」。扫描期强制 duration:0——flip 以视口绝对坐标
      测量，任何上方摘要区高度变化都会让它把列表整体滑动，故扫描期必须让它成为 no-op，
      保证「恰好一次 settle」（async-UI review P1/P2）。
    -->
    <li class="cat" animate:flip={{ duration: scanning ? 0 : 250 }}>
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
          <!-- 折叠层决策辅助：体积占比条（move 6 R1，答「值不值删」） -->
          {#if cat.size > 0}
            <span class="cat-bar" aria-hidden="true">
              <span class="cat-bar-fill" style="width: {fractionOf(cat.size) * 100}%"></span>
            </span>
          {/if}
          <span class="cat-meta">
            <span class="count">{cat.count} 项</span>
            <span class="cat-size">{formatBytes(cat.size)}</span>
          </span>
        {/if}
      </button>

      {#if expanded.has(cat.name) && !scanning}
        <!-- 展开层=审查面孔（move 6 R4）：每项换成「它到底是什么」的形态 -->
        <ul class="reviews">
          {#each cat.items as item (item.path)}
            <li class="review" class:selected={item.selected}>
              <div class="review-main">
                <label class="check">
                  <input
                    type="checkbox"
                    checked={item.selected}
                    onchange={() => onToggle?.(item)}
                    aria-label={item.path}
                  />
                </label>
                <Safety safety={item.safety} />
                <span class="rpath" title={item.path}>{item.path}</span>
                <CopyButton text={item.path} label="复制路径" />
                <span class="size">{formatBytes(item.size)}</span>
              </div>
              <div class="review-detail">
                <EvidenceCard impact={item.impact} recovery={item.recovery} full />
                <button class="finder" onclick={() => reveal(item.path)}>
                  在 Finder 中显示
                </button>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </li>
  {/each}
</ul>

{#if actionError}
  <p class="action-error" role="alert">{actionError}</p>
{/if}

<style>
  .cli-hint {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex-wrap: wrap;
    padding: var(--sp-2) var(--sp-3);
    margin-bottom: var(--sp-2);
    border: 1px dashed var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-raised);
  }
  .cli-label {
    font-size: 0.8em;
    color: var(--ink-muted);
  }
  .cli-cmd {
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-primary);
    padding: 2px var(--sp-2);
    background: var(--surface-base);
    border-radius: 4px;
  }
  .cli-note {
    font-size: 0.78em;
    color: var(--ink-faint);
  }
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
    flex: 0 1 auto;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* 折叠层占比条：低饱和、随分类色系（accent），弹性占据中段 */
  .cat-bar {
    flex: 1 1 auto;
    height: 6px;
    min-width: 40px;
    border-radius: 3px;
    background: color-mix(in oklch, var(--accent) 12%, transparent);
    overflow: hidden;
  }
  .cat-bar-fill {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: 3px;
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
    flex: 1 1 auto;
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
  @media (prefers-reduced-motion: reduce) {
    .skeleton {
      animation: none;
    }
  }
  .reviews {
    list-style: none;
    margin: 0;
    padding: 0 var(--sp-2) var(--sp-2);
    border-top: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .review {
    padding: var(--sp-2) var(--sp-1);
    border-radius: var(--radius);
  }
  .review:hover {
    background: var(--surface-overlay);
  }
  .review.selected {
    background: color-mix(in oklch, var(--accent) 10%, transparent);
  }
  .review-main {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
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
  /* 审查态完整路径：等宽、可选中、末尾优先可见，长则换行 */
  .rpath {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-primary);
    word-break: break-all;
    user-select: text;
  }
  .size {
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-muted);
  }
  /* 第二行：证据全文 + Finder 出口，缩进对齐路径列 */
  .review-detail {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-3);
    padding-left: calc(15px + var(--sp-3));
    margin-top: var(--sp-1);
  }
  .finder {
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 0.78em;
    padding: 2px var(--sp-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-base);
    color: var(--accent-explore);
    cursor: pointer;
  }
  .finder:hover {
    border-color: var(--accent-explore);
  }
  .action-error {
    margin: var(--sp-2) 0 0;
    color: var(--state-danger);
    font-size: 0.8em;
  }
</style>
