<script lang="ts">
  import { scanClean, clean, cancelScan, type SafetyLevel } from "../lib/ipc";
  import { formatBytes } from "../lib/format";
  import Safety from "../lib/Safety.svelte";
  import ConfirmDelete from "../lib/ConfirmDelete.svelte";
  import type { ConfirmItem } from "../lib/ConfirmDelete.svelte";

  type LiveItem = {
    path: string;
    size: number;
    safety: SafetyLevel;
    category: string;
    impact: string;
    recovery: string;
    selected: boolean;
  };

  type Phase = "idle" | "scanning" | "results" | "cleaning" | "done";

  let phase = $state<Phase>("idle");
  let items = $state<LiveItem[]>([]);
  let currentPath = $state("");
  let error = $state<string | null>(null);
  let skipped = $state<string[]>([]);
  let showSkipped = $state(false);

  let confirmItems = $state<ConfirmItem[] | null>(null);

  let cleaningPath = $state("");
  let freed = $state(0);
  let cleanedCount = $state(0);

  // 派生：分类分组（发现顺序内累加），总大小/已选统计
  const groups = $derived.by(() => {
    const map = new Map<string, LiveItem[]>();
    for (const it of items) {
      const arr = map.get(it.category);
      if (arr) arr.push(it);
      else map.set(it.category, [it]);
    }
    return [...map.entries()].map(([name, list]) => ({
      name,
      items: list,
      total: list.reduce((s, i) => s + i.size, 0),
    }));
  });
  const selectedItems = $derived(items.filter((i) => i.selected));
  const selectedSize = $derived(selectedItems.reduce((s, i) => s + i.size, 0));
  const scannedSize = $derived(items.reduce((s, i) => s + i.size, 0));

  async function startScan() {
    phase = "scanning";
    items = [];
    skipped = [];
    error = null;
    currentPath = "";
    // core 的 Found.size 是同一 (category, path) 的**增量 delta**（scanner.rs:736，
    // 大目录会分多次 flush 重复上报同一基路径）；必须按 (category, path) 合并累加，
    // 否则会产生重复行、Svelte 重复 key、选择碎片化（R-review，对齐 TUI 的合并语义）。
    const indexByKey = new Map<string, number>();
    try {
      await scanClean((e) => {
        if (typeof e === "string") return; // "Complete"
        if ("Scanning" in e) {
          currentPath = e.Scanning.path;
        } else if ("Found" in e) {
          const f = e.Found;
          const key = `${f.category}\u0000${f.path}`;
          const idx = indexByKey.get(key);
          if (idx !== undefined) {
            // 已存在同一基路径：累加 delta（不改动已计算的预选/元数据）。
            items[idx].size += f.size;
          } else {
            // 首次出现：建项。默认预选：非 Risky 且 preselect（Risky 永不预选）。
            indexByKey.set(key, items.length);
            items.push({
              path: f.path,
              size: f.size,
              safety: f.safety,
              category: f.category,
              impact: f.impact,
              recovery: f.recovery,
              selected: f.safety !== "Risky" && f.preselect,
            });
          }
        } else if ("SkippedNoPermission" in e) {
          skipped.push(e.SkippedNoPermission.path);
        } else if ("Error" in e) {
          error = e.Error;
        }
      });
      phase = "results";
    } catch (err) {
      // 取消也走这里；有内容则停在结果，否则回 idle
      if (items.length > 0) {
        phase = "results";
      } else {
        phase = "idle";
        if (error === null) error = String(err);
      }
    }
  }

  function cancel() {
    void cancelScan();
  }

  function toggle(item: LiveItem) {
    item.selected = !item.selected;
  }

  function openConfirm() {
    if (selectedItems.length === 0) return;
    confirmItems = selectedItems.map((i) => ({
      path: i.path,
      size: i.size,
      safety: i.safety,
    }));
  }

  async function runClean(token: string) {
    const paths = (confirmItems ?? []).map((i) => i.path);
    confirmItems = null;
    if (paths.length === 0) return;
    phase = "cleaning";
    error = null; // 清空上一轮扫描期可能残留的错误横幅（R-review）
    freed = 0;
    cleanedCount = 0;
    cleaningPath = "";
    try {
      await clean(paths, token, (e) => {
        if (typeof e === "string") return;
        if ("CleaningFile" in e) {
          cleaningPath = e.CleaningFile.path;
        } else if ("CleaningDone" in e) {
          freed = e.CleaningDone.freed;
          cleanedCount = e.CleaningDone.count;
        } else if ("Error" in e) {
          error = e.Error;
        }
      });
    } catch (err) {
      error = String(err);
    }
    phase = "done";
  }

  function reset() {
    phase = "idle";
    items = [];
    error = null;
  }
</script>

<div class="clean">
  {#if phase === "idle"}
    <div class="hero">
      <p>扫描系统与浏览器缓存，安全清理可自动补回的空间。</p>
      <button class="primary" onclick={startScan}>开始扫描</button>
    </div>
  {/if}

  {#if phase === "scanning"}
    <div class="statusbar">
      <span class="spinner" aria-hidden="true">⠋</span>
      <span class="scanning-path" title={currentPath}>{currentPath || "扫描中…"}</span>
      <span class="running-total">{formatBytes(scannedSize)} · {items.length} 项</span>
      <button class="danger-ghost" onclick={cancel}>取消</button>
    </div>
  {/if}

  {#if error && phase !== "cleaning"}
    <p class="error" role="alert">扫描出错：{error}</p>
  {/if}

  {#if (phase === "scanning" || phase === "results") && groups.length > 0}
    <div class="results">
      {#each groups as g (g.name)}
        <section class="group">
          <header class="group-head">
            <span class="group-name">{g.name}</span>
            <span class="group-size">{formatBytes(g.total)}</span>
          </header>
          <ul class="rows">
            {#each g.items as item (item.path)}
              <li class="row" class:selected={item.selected}>
                <label class="check">
                  <input
                    type="checkbox"
                    checked={item.selected}
                    onchange={() => toggle(item)}
                    aria-label={item.path}
                  />
                </label>
                <Safety safety={item.safety} />
                <span class="path" title={item.path}>{item.path}</span>
                <span class="size">{formatBytes(item.size)}</span>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}

  {#if phase === "results" && groups.length === 0}
    <p class="empty">未发现可清理项——系统很干净。</p>
  {/if}

  {#if phase === "results"}
    {#if skipped.length > 0}
      <div class="skipped">
        <button class="link" onclick={() => (showSkipped = !showSkipped)}>
          因权限跳过 {skipped.length} 项 {showSkipped ? "▼" : "▶"}
        </button>
        {#if showSkipped}
          <ul class="skipped-list">
            {#each skipped as p (p)}
              <li title={p}>{p}</li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}

    <div class="actionbar">
      <span class="summary">
        已选 {selectedItems.length} 项 · {formatBytes(selectedSize)}
      </span>
      <div class="btns">
        <button onclick={startScan}>重新扫描</button>
        <button
          class="delete"
          disabled={selectedItems.length === 0}
          onclick={openConfirm}
        >
          删除选中
        </button>
      </div>
    </div>
  {/if}

  {#if phase === "cleaning"}
    <div class="statusbar">
      <span class="spinner" aria-hidden="true">⠋</span>
      <span class="scanning-path" title={cleaningPath}>{cleaningPath || "清理中…"}</span>
    </div>
  {/if}

  {#if phase === "done"}
    <div class="hero">
      <p class="freed">已释放 <strong>{formatBytes(freed)}</strong>（{cleanedCount} 项，已移入废纸篓）</p>
      {#if error}<p class="error">{error}</p>{/if}
      <button class="primary" onclick={reset}>完成</button>
    </div>
  {/if}
</div>

{#if confirmItems}
  <ConfirmDelete
    items={confirmItems}
    onConfirm={runClean}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .clean {
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
  .empty {
    padding: var(--sp-6) 0;
    text-align: center;
    color: var(--ink-muted);
  }
  .freed {
    font-size: 1.1rem;
    color: var(--ink-primary);
  }
  .freed strong {
    color: var(--state-success);
    font-family: var(--font-mono);
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
  .spinner {
    color: var(--state-activity);
    animation: spin 0.8s steps(10) infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .scanning-path {
    flex: 1 1 auto;
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .running-total {
    font-family: var(--font-mono);
    color: var(--state-success);
    flex: 0 0 auto;
  }
  .results {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .group {
    margin-bottom: var(--sp-4);
  }
  .group-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: var(--sp-2) var(--sp-1);
    border-bottom: 1px solid var(--border-subtle);
    position: sticky;
    top: 0;
    background: var(--surface-base);
  }
  .group-name {
    color: var(--accent);
    font-weight: 600;
  }
  .group-size {
    font-family: var(--font-mono);
    color: var(--ink-muted);
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
  }
  .row:hover {
    background: var(--surface-raised);
  }
  .row.selected {
    background: color-mix(in oklch, var(--accent) 12%, transparent);
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
  .path {
    flex: 1 1 auto;
    font-family: var(--font-mono);
    font-size: 0.85em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .size {
    font-family: var(--font-mono);
    color: var(--ink-muted);
    flex: 0 0 auto;
  }
  .skipped {
    padding: var(--sp-2) 0;
    border-top: 1px solid var(--border-subtle);
  }
  .skipped-list {
    list-style: none;
    margin: var(--sp-2) 0 0;
    padding: 0;
    max-height: 140px;
    overflow-y: auto;
  }
  .skipped-list li {
    font-family: var(--font-mono);
    font-size: 0.8em;
    color: var(--ink-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .link {
    background: none;
    border: none;
    color: var(--accent);
    padding: 0;
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 0.85em;
  }
  .error {
    color: var(--state-danger);
    font-family: var(--font-mono);
    font-size: 0.85em;
  }
</style>
