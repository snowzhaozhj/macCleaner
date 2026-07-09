<script lang="ts">
  import { flip } from "svelte/animate";
  import {
    analyze,
    classifyMarked,
    deleteMarked,
    cancelScan,
    openTrash,
    userHome,
    type DirNode,
  } from "../lib/ipc";
  import { formatBytes, dirSegments } from "../lib/format";
  import { withViewTransition } from "../lib/transition";
  import { nextToast, dismissToast, type ToastState } from "../lib/toast";
  import Shell from "../lib/Shell.svelte";
  import SummaryHeader from "../lib/SummaryHeader.svelte";
  import UndoToast from "../lib/UndoToast.svelte";
  import ConfirmDelete from "../lib/ConfirmDelete.svelte";
  import type { ConfirmItem } from "../lib/ConfirmDelete.svelte";

  type Phase = "idle" | "analyzing" | "ready" | "deleting";

  let phase = $state<Phase>("idle");
  let tree = $state<DirNode | null>(null);
  let navPaths = $state<string[]>([]); // 从根向下逐层的绝对路径（不含根）
  let marked = $state<Map<string, number>>(new Map()); // path → size（用于确认清单）
  let fileCount = $state(0);
  let totalSize = $state(0);
  let error = $state<string | null>(null);
  let confirmItems = $state<ConfirmItem[] | null>(null);
  let deletingPath = $state("");
  let toast = $state<ToastState>(null);

  const analyzing = $derived(phase === "analyzing");

  // 当前所在节点：按 navPaths 从 tree 走下去（存储序，健壮于剪树后的引用变化）
  const currentNode = $derived.by(() => {
    if (!tree) return null;
    let node = tree;
    for (const p of navPaths) {
      const next = node.children.find((c) => c.path === p);
      if (!next) break;
      node = next;
    }
    return node;
  });

  // 显示序：当前层按 size 降序（DESIGN.md §6.0 分析器行）
  const sortedChildren = $derived.by(() => {
    if (!currentNode) return [];
    return [...currentNode.children].sort((a, b) => b.size - a.size);
  });
  const maxChildSize = $derived(
    sortedChildren.length > 0 ? sortedChildren[0].size : 0,
  );

  // 首屏主数字（R6）：扫描期取实时累加总量，就绪后定格为当前导航层总占用。
  const headerAmount = $derived(
    analyzing ? totalSize : (currentNode?.size ?? 0),
  );
  // move 5 空间地理分区（R4/R5）：分段横条呈现**当前层** top 消费者；扫描期无树→空段。
  const levelSegments = $derived(
    currentNode && !analyzing ? dirSegments(currentNode.children) : [],
  );

  // 面包屑：根 + 每层 { name, paths }（点击回溯到该层）
  const trail = $derived.by(() => {
    const out: { name: string; paths: string[] }[] = [];
    if (!tree) return out;
    out.push({ name: tree.name || tree.path, paths: [] });
    let node = tree;
    const acc: string[] = [];
    for (const p of navPaths) {
      const next = node.children.find((c) => c.path === p);
      if (!next) break;
      acc.push(p);
      out.push({ name: next.name, paths: [...acc] });
      node = next;
    }
    return out;
  });

  const markedItems = $derived(
    [...marked.entries()].map(([path, size]) => ({ path, size })),
  );
  const markedSize = $derived(markedItems.reduce((s, i) => s + i.size, 0));

  function setPhase(p: Phase) {
    withViewTransition(() => {
      phase = p;
    });
  }

  async function startAnalyze() {
    tree = null;
    navPaths = [];
    marked = new Map();
    fileCount = 0;
    totalSize = 0;
    error = null;
    setPhase("analyzing");
    try {
      const root = await userHome();
      // 后端 analyze 返回 finalize 后的完整树 + 扫描期 Progress 累加（增量树流式为后续版本）。
      tree = await analyze(root, (e) => {
        if (typeof e === "string") return; // "Finished"
        if ("Progress" in e) {
          fileCount = e.Progress.file_count;
          totalSize = e.Progress.total_size;
        }
      });
      setPhase("ready");
    } catch (err) {
      // 取消后已有部分树则进 ready 展示；否则回 idle 并报错。
      if (tree) setPhase("ready");
      else {
        error = String(err);
        setPhase("idle");
      }
    }
  }

  function cancel() {
    void cancelScan();
  }

  function enter(node: DirNode) {
    if (node.is_file || node.children.length === 0) return;
    navPaths = [...navPaths, node.path];
  }

  function gotoTrail(paths: string[]) {
    navPaths = paths;
  }

  function toggleMark(node: DirNode) {
    const next = new Map(marked);
    if (next.has(node.path)) next.delete(node.path);
    else next.set(node.path, node.size);
    marked = next;
  }

  async function openConfirm() {
    if (marked.size === 0) return;
    // 分析器项无规则元数据：打开确认弹窗前按路径回查安全分级，让 Risky 路径
    // （Docker 卷/Xcode Archives 等）在弹窗显示危险三通道并触发 type-to-confirm（R9）。
    const items = markedItems;
    let safetyByPath = new Map<string, "Safe" | "Moderate" | "Risky">();
    try {
      const classified = await classifyMarked(items.map((i) => i.path));
      safetyByPath = new Map(classified.map((c) => [c.path, c.safety]));
    } catch (err) {
      // 回查失败降级：保守地把全部项按 Risky 呈现（强制 type-to-confirm），不静默放行。
      error = `安全分级查询失败：${String(err)}`;
      for (const i of items) safetyByPath.set(i.path, "Risky");
    }
    confirmItems = items.map((i) => ({
      path: i.path,
      size: i.size,
      safety: safetyByPath.get(i.path) ?? "Safe",
    }));
  }

  // 原地剪树：移除 deleted 路径的节点并沿链回减各祖先 size（不重新 analyze）
  function pruneTree(node: DirNode, deleted: Set<string>): number {
    let removed = 0;
    const kept: DirNode[] = [];
    for (const c of node.children) {
      if (deleted.has(c.path)) {
        removed += c.size;
      } else {
        kept.push(c);
      }
    }
    node.children = kept;
    for (const c of node.children) {
      removed += pruneTree(c, deleted);
    }
    node.size -= removed;
    return removed;
  }

  async function runDelete(token: string) {
    const paths = (confirmItems ?? []).map((i) => i.path);
    confirmItems = null;
    if (paths.length === 0) return;
    error = null; // 清空上一轮分析期可能残留的错误横幅
    deletingPath = "";
    setPhase("deleting");
    let deleted: string[] = [];
    let freed = 0;
    try {
      await deleteMarked(paths, token, (e) => {
        if (typeof e === "string") return;
        if ("CleaningFile" in e) {
          deletingPath = e.CleaningFile.path;
        } else if ("CleaningDone" in e) {
          deleted = e.CleaningDone.deleted_paths;
          freed = e.CleaningDone.freed;
        } else if ("Error" in e) {
          error = e.Error;
        }
      });
    } catch (err) {
      error = String(err);
    }
    if (tree && deleted.length > 0) {
      const set = new Set(deleted);
      pruneTree(tree, set);
      tree = { ...tree }; // 触发依赖 currentNode/trail/segments 的重算
      // 删除祖先目录会连带移除其整棵子树；marked 里被独立标记的**后代**路径也随之失效，
      // 必须一并清出，否则残留陈旧标记（计数虚高、确认列表出现已不存在的路径）。
      const nextMarked = new Map(marked);
      for (const key of [...nextMarked.keys()]) {
        if (deleted.some((d) => key === d || key.startsWith(`${d}/`))) {
          nextMarked.delete(key);
        }
      }
      marked = nextMarked;
      // 与 Clean 一致的诚实提示：已移废纸篓、可在访达恢复（R10，单实例）。
      toast = nextToast(toast, deleted.length, freed);
    }
    setPhase("ready");
  }

  function restoreInFinder() {
    void openTrash();
  }

  function barWidth(size: number): number {
    return maxChildSize > 0 ? Math.max(2, (size / maxChildSize) * 100) : 0;
  }

  // toast 自动消失（6s）；新一次删除会重置计时（seq 变化触发 effect 重跑）。
  $effect(() => {
    const t = toast;
    if (!t) return;
    const seq = t.seq;
    const timer = setTimeout(() => {
      if (toast?.seq === seq) toast = dismissToast();
    }, 6000);
    return () => clearTimeout(timer);
  });

  const LARGE_FILE = 100 * 1024 * 1024; // 100 MiB
  const SKELETON_ROWS = 6;
</script>

<Shell>
  {#snippet summary()}
    {#if phase !== "idle"}
      <SummaryHeader
        lead="占用"
        amount={headerAmount}
        segments={levelSegments}
        scanning={analyzing}
      />
      <!-- 扫描期不在摘要区显示错误横幅：其高度变化会推动列表位移（防跳变）；错误在完成后呈现 -->
      {#if error && !analyzing}<p class="error" role="alert">出错：{error}</p>{/if}
    {/if}
  {/snippet}

  {#snippet list()}
    {#if phase === "idle"}
      <p class="hint">
        分析主目录磁盘占用，按体积降序逐层导航，标记大项后可移入废纸篓。
      </p>
    {:else}
      {#if trail.length > 0 && !analyzing}
        <nav class="breadcrumb" aria-label="路径">
          {#each trail as crumb, i (crumb.name + i)}
            {#if i > 0}<span class="sep">/</span>{/if}
            <button class="crumb" onclick={() => gotoTrail(crumb.paths)}>
              {crumb.name}
            </button>
          {/each}
          <span class="crumb-size">{formatBytes(currentNode?.size ?? 0)}</span>
        </nav>
      {/if}

      {#if analyzing}
        <ul class="rows" aria-hidden="true">
          {#each Array(SKELETON_ROWS) as _, i (i)}
            <li class="row skeleton-row">
              <span class="sk sk-check"></span>
              <span class="sk sk-name"></span>
              <span class="sk sk-bar"></span>
              <span class="sk sk-size"></span>
            </li>
          {/each}
        </ul>
      {:else}
        <ul class="rows">
          {#each sortedChildren as node (node.path)}
            {@const isMarked = marked.has(node.path)}
            {@const isLarge = node.is_file && node.size > LARGE_FILE}
            {@const canEnter = !node.is_file && node.children.length > 0}
            <li class="row" class:marked={isMarked} animate:flip={{ duration: 200 }}>
              <label class="check">
                <input
                  type="checkbox"
                  checked={isMarked}
                  onchange={() => toggleMark(node)}
                  aria-label={node.path}
                />
              </label>
              <button
                class="enter"
                class:invisible={!canEnter}
                disabled={!canEnter}
                onclick={() => enter(node)}
                aria-label="进入 {node.name}"
              >
                <svg viewBox="0 0 10 10" width="10" height="10" aria-hidden="true">
                  <path d="M3 2 L7 5 L3 8" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
                </svg>
              </button>
              <span
                class="name"
                class:dir={!node.is_file}
                class:large={isLarge}
                class:struck={isMarked}
                title={node.path}
              >
                {#if isLarge}<span class="warn-glyph" aria-hidden="true">⚠</span>{/if}{node.name}
              </span>
              <span class="bar-wrap" aria-hidden="true">
                <span class="bar" style="width: {barWidth(node.size)}%"></span>
              </span>
              <span class="size" class:struck={isMarked}>{formatBytes(node.size)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if phase === "idle"}
      <div class="btns">
        <button class="primary" onclick={startAnalyze}>分析主目录</button>
      </div>
    {:else if phase === "analyzing"}
      <div class="scan-actions">
        <span class="prog-text" aria-live="polite">
          分析中 · {fileCount} 个文件 · {formatBytes(totalSize)}
        </span>
        <button class="ghost-danger" onclick={cancel}>取消</button>
      </div>
    {:else if phase === "deleting"}
      <div class="scan-actions">
        <span class="prog-text mono" title={deletingPath}>
          {deletingPath || "移入废纸篓中…"}
        </span>
      </div>
    {:else}
      <!-- ready -->
      <div class="btns">
        <span class="marked-summary">
          已标记 {marked.size} 项 · {formatBytes(markedSize)}
        </span>
        <button onclick={startAnalyze}>重新分析</button>
        <button
          class="primary delete"
          disabled={marked.size === 0}
          onclick={openConfirm}
        >
          删除标记
        </button>
      </div>
    {/if}
  {/snippet}
</Shell>

{#if toast}
  {#key toast.seq}
    <UndoToast
      count={toast.count}
      freed={toast.freed}
      onRestore={restoreInFinder}
      onDismiss={() => (toast = dismissToast())}
    />
  {/key}
{/if}

{#if confirmItems}
  <ConfirmDelete
    items={confirmItems}
    onConfirm={runDelete}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .hint {
    margin: auto;
    max-width: 42ch;
    text-align: center;
    color: var(--ink-muted);
    font-size: 0.9em;
    line-height: 1.6;
  }
  .error {
    margin: var(--sp-2) 0 0;
    color: var(--state-danger);
    font-size: 0.85em;
  }
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex-wrap: wrap;
    padding: 0 0 var(--sp-2);
    border-bottom: 1px solid var(--border-subtle);
    margin-bottom: var(--sp-2);
  }
  .sep {
    color: var(--ink-faint);
  }
  .crumb {
    background: none;
    border: none;
    color: var(--accent-explore);
    cursor: pointer;
    padding: 0 var(--sp-1);
    font-family: var(--font-ui);
    font-size: 0.9em;
  }
  .crumb:hover {
    text-decoration: underline;
  }
  .crumb-size {
    margin-left: auto;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: var(--row-height);
    padding: 0 var(--sp-1);
    border-radius: var(--radius);
  }
  .row:hover {
    background: var(--surface-raised);
  }
  .row.marked {
    background: color-mix(in oklch, var(--state-danger) 14%, transparent);
  }
  .check {
    display: flex;
    flex: 0 0 auto;
  }
  .check input {
    accent-color: var(--accent-explore);
    width: 15px;
    height: 15px;
    cursor: pointer;
  }
  .enter {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--accent-explore);
    cursor: pointer;
    padding: 0;
    width: 16px;
    height: 16px;
  }
  .enter.invisible {
    visibility: hidden;
    cursor: default;
  }
  /* R7：名列改用 UI 字体 + 弹性宽度，脱离终端等宽固定网格 */
  .name {
    flex: 1 1 auto;
    min-width: 8ch;
    font-family: var(--font-ui);
    font-size: 0.9em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--ink-primary);
  }
  .name.dir {
    color: var(--accent-explore);
  }
  .name.large {
    color: var(--state-warning);
  }
  .name.struck {
    text-decoration: line-through;
    color: var(--state-danger);
  }
  .warn-glyph {
    margin-right: var(--sp-1);
  }
  .bar-wrap {
    flex: 0 1 28%;
    height: 8px;
    background: color-mix(in oklch, var(--accent-explore) 12%, transparent);
    border-radius: 4px;
    overflow: hidden;
    min-width: 40px;
  }
  .bar {
    display: block;
    height: 100%;
    background: var(--accent-explore);
    border-radius: 4px;
  }
  .size {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-muted);
    flex: 0 0 auto;
    width: 9ch;
    text-align: right;
  }
  .size.struck {
    text-decoration: line-through;
    color: var(--state-danger);
  }
  /* 扫描期骨架行：与真实行同高，避免就绪时列表区高度突变（防跳变） */
  .skeleton-row {
    pointer-events: none;
  }
  .sk {
    display: block;
    height: 10px;
    border-radius: 4px;
    background: var(--surface-raised);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .sk-check {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
  }
  .sk-name {
    flex: 1 1 auto;
  }
  .sk-bar {
    flex: 0 1 28%;
    height: 8px;
  }
  .sk-size {
    width: 9ch;
    flex: 0 0 auto;
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 0.5;
    }
    50% {
      opacity: 0.9;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sk {
      animation: none;
    }
  }
  .scan-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
  }
  .prog-text {
    flex: 1 1 auto;
    color: var(--ink-muted);
    font-size: 0.85em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .prog-text.mono {
    font-family: var(--font-mono);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: var(--sp-3);
  }
  .marked-summary {
    margin-right: auto;
    font-size: 0.85em;
    color: var(--ink-muted);
    font-variant-numeric: tabular-nums;
  }
  button {
    font-family: var(--font-ui);
    font-size: 0.9em;
    padding: var(--sp-2) var(--sp-4);
    border-radius: var(--radius);
    border: 1px solid var(--border-subtle);
    background: var(--surface-raised);
    color: var(--ink-primary);
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    border-color: var(--ink-muted);
  }
  button:disabled {
    color: var(--ink-faint);
    cursor: not-allowed;
  }
  .primary {
    border-color: var(--accent);
    color: var(--accent);
    font-weight: 600;
  }
  .delete {
    border-color: var(--state-danger);
    color: var(--state-danger);
  }
  .ghost-danger {
    flex: 0 0 auto;
    border-color: var(--state-danger);
    color: var(--state-danger);
    padding: var(--sp-1) var(--sp-3);
  }
  /* enter/crumb 无边框，hover 不加边框 */
  .enter:hover:not(:disabled),
  .crumb:hover {
    border: none;
  }
</style>
