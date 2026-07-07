<script lang="ts">
  import {
    analyze,
    classifyMarked,
    deleteMarked,
    cancelScan,
    userHome,
    type DirNode,
  } from "../lib/ipc";
  import { formatBytes } from "../lib/format";
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

  async function startAnalyze() {
    phase = "analyzing";
    tree = null;
    navPaths = [];
    marked = new Map();
    fileCount = 0;
    totalSize = 0;
    error = null;
    try {
      const root = await userHome();
      tree = await analyze(root, (e) => {
        if (typeof e === "string") return; // "Finished"
        if ("Progress" in e) {
          fileCount = e.Progress.file_count;
          totalSize = e.Progress.total_size;
        }
      });
      phase = "ready";
    } catch (err) {
      if (tree) phase = "ready";
      else {
        phase = "idle";
        error = String(err);
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
    // （Docker 卷/Xcode Archives 等）在弹窗显示危险三通道并触发 type-to-confirm（R-review codex-P1）。
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
    phase = "deleting";
    error = null; // 清空上一轮分析期可能残留的错误横幅（R-review）
    deletingPath = "";
    let deleted: string[] = [];
    try {
      await deleteMarked(paths, token, (e) => {
        if (typeof e === "string") return;
        if ("CleaningFile" in e) {
          deletingPath = e.CleaningFile.path;
        } else if ("CleaningDone" in e) {
          deleted = e.CleaningDone.deleted_paths;
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
      tree = { ...tree }; // 触发依赖 currentNode/trail 的重算
      // 删除祖先目录会连带移除其整棵子树；marked 里被独立标记的**后代**路径也随之失效，
      // 必须一并清出，否则残留陈旧标记（计数虚高、确认列表出现已不存在的路径，R-review codex-P2）。
      const nextMarked = new Map(marked);
      for (const key of [...nextMarked.keys()]) {
        if (deleted.some((d) => key === d || key.startsWith(`${d}/`))) {
          nextMarked.delete(key);
        }
      }
      marked = nextMarked;
    }
    phase = "ready";
  }

  function barWidth(size: number): number {
    return maxChildSize > 0 ? Math.max(2, (size / maxChildSize) * 100) : 0;
  }

  const LARGE_FILE = 100 * 1024 * 1024; // 100 MiB
</script>

<div class="analyze">
  {#if phase === "idle"}
    <div class="hero">
      <p>分析主目录磁盘占用，按体积降序导航，标记后可移入废纸篓。</p>
      <button class="primary" onclick={startAnalyze}>分析主目录</button>
      {#if error}<p class="error">{error}</p>{/if}
    </div>
  {/if}

  {#if phase === "analyzing"}
    <div class="statusbar">
      <span class="spinner" aria-hidden="true">⠋</span>
      <span class="prog">分析中… {fileCount} 个文件 · {formatBytes(totalSize)}</span>
      <button class="danger-ghost" onclick={cancel}>取消</button>
    </div>
  {/if}

  {#if (phase === "ready" || phase === "deleting") && tree}
    <nav class="breadcrumb" aria-label="路径">
      {#each trail as crumb, i (crumb.name + i)}
        {#if i > 0}<span class="sep">/</span>{/if}
        <button class="crumb" onclick={() => gotoTrail(crumb.paths)}>
          {crumb.name}
        </button>
      {/each}
      <span class="crumb-size">{formatBytes(currentNode?.size ?? 0)}</span>
    </nav>

    <ul class="rows">
      {#each sortedChildren as node (node.path)}
        {@const isMarked = marked.has(node.path)}
        {@const isLarge = node.is_file && node.size > LARGE_FILE}
        {@const canEnter = !node.is_file && node.children.length > 0}
        <li class="row" class:marked={isMarked}>
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
          >▶</button>
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

    <div class="actionbar">
      <span class="summary">
        已标记 {marked.size} 项 · {formatBytes(
          markedItems.reduce((s, i) => s + i.size, 0),
        )}
      </span>
      <div class="btns">
        <button onclick={startAnalyze}>重新分析</button>
        <button class="delete" disabled={marked.size === 0} onclick={openConfirm}>
          删除标记
        </button>
      </div>
    </div>

    {#if phase === "deleting"}
      <div class="statusbar deleting">
        <span class="spinner" aria-hidden="true">⠋</span>
        <span class="prog" title={deletingPath}>{deletingPath || "删除中…"}</span>
      </div>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  {/if}
</div>

{#if confirmItems}
  <ConfirmDelete
    items={confirmItems}
    onConfirm={runDelete}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .analyze {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .hero {
    margin: auto;
    text-align: center;
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    align-items: center;
    color: var(--ink-muted);
  }
  .statusbar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface-raised);
    border-radius: var(--radius);
    margin-bottom: var(--sp-3);
  }
  .statusbar.deleting {
    margin: var(--sp-3) 0 0;
  }
  .spinner {
    color: var(--state-activity);
    animation: spin 0.8s steps(10) infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .prog {
    flex: 1 1 auto;
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex-wrap: wrap;
    padding: var(--sp-2) 0;
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
    font-family: var(--font-mono);
    font-size: 0.9em;
  }
  .crumb:hover {
    text-decoration: underline;
  }
  .crumb-size {
    margin-left: auto;
    font-family: var(--font-mono);
    color: var(--ink-muted);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-height: var(--row-height);
    padding: 0 var(--sp-1);
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
    background: none;
    border: none;
    color: var(--accent-explore);
    cursor: pointer;
    font-size: 0.75em;
    padding: 0;
    width: 1em;
  }
  .enter.invisible {
    visibility: hidden;
    cursor: default;
  }
  .name {
    flex: 0 1 34ch;
    min-width: 8ch;
    font-family: var(--font-mono);
    font-size: 0.85em;
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
    flex: 1 1 auto;
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
    color: var(--ink-muted);
    flex: 0 0 auto;
    width: 9ch;
    text-align: right;
  }
  .size.struck {
    text-decoration: line-through;
    color: var(--state-danger);
  }
  .actionbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--sp-4);
    padding: var(--sp-3) 0 0;
    border-top: 1px solid var(--border-subtle);
  }
  .summary {
    font-family: var(--font-mono);
    color: var(--ink-primary);
  }
  .btns {
    display: flex;
    gap: var(--sp-2);
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
  }
  .delete {
    border-color: var(--state-danger);
    color: var(--state-danger);
    font-weight: 600;
  }
  .danger-ghost {
    border-color: var(--state-danger);
    color: var(--state-danger);
    padding: var(--sp-1) var(--sp-3);
  }
  .error {
    color: var(--state-danger);
    font-family: var(--font-mono);
    font-size: 0.85em;
  }
  /* enter/delete 按钮 hover 不改边框（无边框） */
  .enter:hover:not(:disabled),
  .crumb:hover {
    border: none;
  }
</style>
