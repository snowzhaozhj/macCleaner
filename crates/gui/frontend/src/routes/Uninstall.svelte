<script lang="ts">
  /**
   * 「卸载」路由（move 7 第二段 / plan 021）。**两阶段**应用卸载，镜像 CLI `mc uninstall`：
   * 阶段一列已装应用（可搜索、体积降序），阶段二对选定应用解析 `~/Library` 残留、与 app
   * bundle 合成一份可审查 `ScanResult`，走与 Clean/Purge 一致的安全预选、证据、type-to-confirm、
   * 移废纸篓删除。因流程与 Clean/Purge（单阶段扫→删）不同，显式定义九态相位机——每状态漏
   * 定义即成空白/冻结屏。残留审查复用 StreamingList（knownOrder=[] 动态类目）+ 删除信任链。
   */
  import { onMount } from "svelte";
  import {
    scanUninstall,
    resolveLeftovers,
    uninstall,
    cancelScan,
    openTrash,
    type AppInfo,
    type CleanReport,
  } from "../lib/ipc";
  import {
    computeSegments,
    aggregateByCategory,
    formatBytes,
    shellQuote,
    type LiveItem,
  } from "../lib/format";
  import { withViewTransition } from "../lib/transition";
  import { nextToast, dismissToast, type ToastState } from "../lib/toast";
  import Shell from "../lib/Shell.svelte";
  import SummaryHeader from "../lib/SummaryHeader.svelte";
  import StreamingList from "../lib/StreamingList.svelte";
  import CleanReceipt from "../lib/CleanReceipt.svelte";
  import UndoToast from "../lib/UndoToast.svelte";
  import ConfirmDelete from "../lib/ConfirmDelete.svelte";
  import type { ConfirmItem } from "../lib/ConfirmDelete.svelte";

  // 显式相位机（KTD6）：list→review→delete 无法照搬 Clean/Purge 的 idle/scanning/results。
  type Phase =
    | "listLoading" | "listReady" | "listEmpty" | "listError"
    | "reviewLoading" | "reviewReady" | "reviewError"
    | "deleting" | "done";

  let phase = $state<Phase>("listLoading");
  let apps = $state<AppInfo[]>([]);
  let search = $state("");
  let selectedApp = $state<AppInfo | null>(null);
  let reviewItems = $state<LiveItem[]>([]);
  let error = $state<string | null>(null);

  let confirmItems = $state<ConfirmItem[] | null>(null);
  let cleaningPath = $state("");
  let lastReport = $state<CleanReport | null>(null);
  let toast = $state<ToastState>(null);

  // ---- 阶段一：应用列表派生（搜索过滤 + 体积降序，R3/R4）----
  const filteredApps = $derived.by(() => {
    const q = search.trim().toLowerCase();
    const matched = q
      ? apps.filter(
          (a) =>
            a.name.toLowerCase().includes(q) ||
            (a.bundle_id?.toLowerCase().includes(q) ?? false),
        )
      : apps;
    return [...matched].sort((x, y) => y.size - x.size);
  });
  // 零命中：有应用但搜索无匹配（区别于 listEmpty 真空扫描，AE2）。
  const zeroMatch = $derived(
    search.trim().length > 0 && filteredApps.length === 0 && apps.length > 0,
  );

  // ---- 阶段二：残留审查派生 ----
  const reviewing = $derived(
    phase === "reviewLoading" || phase === "reviewReady" || phase === "reviewError" || phase === "deleting",
  );
  const reviewCats = $derived(aggregateByCategory(reviewItems, [], true));
  const selectedItems = $derived(reviewItems.filter((i) => i.selected));
  const selectedSize = $derived(
    reviewItems.reduce((s, i) => (i.selected ? s + i.size : s), 0),
  );
  const segments = $derived(
    computeSegments(reviewCats.map((c) => ({ ...c, size: c.selectedSize }))),
  );
  // 残留说明（AE4/AE10）：无 bundle_id vs 有 bundle_id 但零残留，两措辞须不同。
  const hasBundleId = $derived(!!selectedApp?.bundle_id);
  const leftoverCount = $derived(reviewItems.filter((i) => i.category !== "应用").length);
  const noBundleNote = $derived(phase === "reviewReady" && !hasBundleId);
  const noLeftoverNote = $derived(phase === "reviewReady" && hasBundleId && leftoverCount === 0);

  function setPhase(p: Phase) {
    withViewTransition(() => {
      phase = p;
    });
  }

  // ---- 阶段一：列应用（进入即扫，F1）----
  async function startListScan() {
    error = null;
    setPhase("listLoading");
    try {
      const result = await scanUninstall();
      apps = result;
      setPhase(result.length > 0 ? "listReady" : "listEmpty");
    } catch (err) {
      error = String(err);
      setPhase("listError");
    }
  }

  // ---- 阶段二：选应用解析残留（F2）----
  async function selectApp(app: AppInfo) {
    selectedApp = app;
    error = null;
    reviewItems = [];
    setPhase("reviewLoading");
    try {
      const result = await resolveLeftovers(app.path, app.bundle_id, app.size);
      reviewItems = result.categories.flatMap((g) =>
        g.items.map((it) => ({
          path: it.path,
          size: it.size,
          safety: it.safety,
          category: it.category,
          impact: it.impact,
          recovery: it.recovery,
          selected: it.selected,
        })),
      );
      setPhase("reviewReady");
    } catch (err) {
      error = String(err);
      setPhase("reviewError");
    }
  }

  // 返回应用列表（R9）：保留列表，清空审查态。
  function backToList() {
    selectedApp = null;
    reviewItems = [];
    error = null;
    setPhase(apps.length > 0 ? "listReady" : "listEmpty");
  }

  function toggle(item: LiveItem) {
    item.selected = !item.selected;
  }

  // ---- 阶段三：删除（纯 Safe/Moderate 直删；含 Risky 走 type-to-confirm，R10）----
  function primaryDelete() {
    if (selectedItems.length === 0) return;
    if (selectedItems.some((i) => i.safety === "Risky")) {
      confirmItems = selectedItems.map((i) => ({
        path: i.path,
        size: i.size,
        safety: i.safety,
        impact: i.impact,
        recovery: i.recovery,
      }));
      return;
    }
    void doUninstall(
      selectedItems.map((i) => i.path),
      "",
    );
  }

  async function doUninstall(paths: string[], token: string) {
    confirmItems = null;
    if (paths.length === 0) return;
    error = null;
    cleaningPath = "";
    const uninstalledPath = selectedApp?.path;
    setPhase("deleting");
    let report: CleanReport | null = null;
    try {
      report = await uninstall(paths, token, (e) => {
        if (typeof e === "string") return;
        if ("CleaningFile" in e) cleaningPath = e.CleaningFile.path;
        else if ("Error" in e) error = e.Error;
      });
    } catch (err) {
      error = String(err);
    }
    if (report) {
      lastReport = report;
      if (report.success_count > 0) {
        toast = nextToast(toast, report.success_count, report.total_freed);
        // 删成功后从缓存剔除已卸载应用（R18/AE9，照 Analyze 删后剪树）：
        // 重选已删应用会对不存在的 bundle 空转，故列表须反映删除。
        if (uninstalledPath) apps = apps.filter((a) => a.path !== uninstalledPath);
      }
    }
    setPhase("done");
  }

  function restoreInFinder() {
    void openTrash();
  }

  // toast 自动消失（6s），同 Clean/Purge。
  $effect(() => {
    const t = toast;
    if (!t) return;
    const seq = t.seq;
    const timer = setTimeout(() => {
      if (toast?.seq === seq) toast = dismissToast();
    }, 6000);
    return () => clearTimeout(timer);
  });

  onMount(() => {
    void startListScan();
    return () => {
      // 仅删除在途时协作取消（uninstall 走 begin_operation）；list/review 是纯查询无取消 flag，
      // 无条件 cancel 会与下一 tab mount 的 begin_operation 竞速。
      if (phase === "deleting") void cancelScan();
    };
  });
</script>

<Shell>
  {#snippet summary()}
    {#if phase === "done" && lastReport}
      <CleanReceipt report={lastReport} onRestore={restoreInFinder} />
    {:else if reviewing}
      <SummaryHeader amount={selectedSize} {segments} scanning={false} />
      {#if selectedApp}
        <p class="review-app" title={selectedApp.path}>
          卸载 <strong>{selectedApp.name}</strong>
          {#if selectedApp.version}<span class="ver">v{selectedApp.version}</span>{/if}
        </p>
      {/if}
      {#if error && phase !== "deleting"}<p class="error" role="alert">出错：{error}</p>{/if}
    {:else}
      <!-- 阶段一 list 相位 -->
      <div class="list-summary">
        <span class="ls-title">已安装应用</span>
        {#if phase === "listReady"}<span class="ls-count">{apps.length} 个</span>{/if}
      </div>
      {#if error}<p class="error" role="alert">出错：{error}</p>{/if}
    {/if}
  {/snippet}

  {#snippet list()}
    {#if phase === "listLoading"}
      <p class="loading" role="status" aria-live="polite">扫描应用中…</p>
      <ul class="skeletons" aria-hidden="true">
        {#each [0, 1, 2, 3] as i (i)}
          <li class="skeleton-row"></li>
        {/each}
      </ul>
    {:else if phase === "listEmpty"}
      <p class="empty">未发现已安装应用（/Applications 与 ~/Applications 均为空或不可读）。</p>
    {:else if phase === "listReady"}
      <div class="search-bar">
        <input
          type="search"
          class="search"
          placeholder="按名称或 bundle ID 搜索…"
          bind:value={search}
          aria-label="搜索应用"
        />
      </div>
      {#if zeroMatch}
        <p class="empty">没有匹配「{search}」的应用。</p>
      {:else}
        <ul class="apps">
          {#each filteredApps as app (app.path)}
            <li class="app">
              <button class="app-row" onclick={() => selectApp(app)}>
                <span class="app-name">{app.name}</span>
                {#if app.version}<span class="app-ver">v{app.version}</span>{/if}
                <span class="app-size">{formatBytes(app.size)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {:else if phase === "reviewLoading"}
      <p class="loading" role="status" aria-live="polite">解析残留中…</p>
      <ul class="skeletons" aria-hidden="true">
        {#each [0, 1] as i (i)}
          <li class="skeleton-row"></li>
        {/each}
      </ul>
    {:else if phase === "reviewReady"}
      {#if noBundleNote}
        <p class="note">未能解析残留（应用无 bundle 标识），仅可移除应用本体。</p>
      {:else if noLeftoverNote}
        <p class="note">未发现残留，仅移除应用本体。</p>
      {/if}
      <StreamingList
        items={reviewItems}
        scanning={false}
        onToggle={toggle}
        cliCommand={`mc uninstall ${shellQuote(selectedApp?.name ?? "")}`}
        cliNote="卸载该应用并清理其残留"
      />
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if phase === "listLoading" || phase === "reviewLoading"}
      <div class="scan-actions">
        <span class="prog-text">{phase === "listLoading" ? "扫描中…" : "解析中…"}</span>
      </div>
    {:else if phase === "deleting"}
      <div class="scan-actions">
        <span class="prog-text mono" title={cleaningPath}>
          {cleaningPath || "移入废纸篓中…"}
        </span>
      </div>
    {:else if phase === "done"}
      <div class="btns">
        <button class="primary" onclick={backToList}>返回应用列表</button>
      </div>
    {:else if phase === "reviewReady" || phase === "reviewError"}
      <div class="btns">
        <button onclick={backToList}>返回列表</button>
        {#if phase === "reviewReady"}
          <button
            class="primary delete"
            disabled={selectedItems.length === 0}
            onclick={primaryDelete}
          >
            移入废纸篓 · 释放 {formatBytes(selectedSize)}
          </button>
        {/if}
      </div>
    {:else}
      <!-- listReady / listEmpty / listError -->
      <div class="btns">
        <button onclick={startListScan}>重新扫描应用</button>
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
    onConfirm={(token) => doUninstall(confirmItems?.map((i) => i.path) ?? [], token)}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .list-summary {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
  }
  .ls-title {
    font-size: 1.1em;
    font-weight: 600;
    color: var(--ink-primary);
  }
  .ls-count {
    color: var(--ink-muted);
    font-variant-numeric: tabular-nums;
  }
  .review-app {
    margin: var(--sp-2) 0 0;
    color: var(--ink-muted);
    font-size: 0.9em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .review-app .ver {
    color: var(--ink-faint);
    font-family: var(--font-mono);
    font-size: 0.85em;
    margin-left: var(--sp-2);
  }
  .search-bar {
    padding: var(--sp-2) 0;
  }
  .search {
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius);
    border: 1px solid var(--border-subtle);
    background: var(--surface-base);
    color: var(--ink-primary);
    font-family: var(--font-ui);
    font-size: 0.9em;
  }
  .search:focus {
    outline: none;
    border-color: var(--accent);
  }
  .apps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    max-height: 320px;
    overflow-y: auto;
  }
  .app-row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-3);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-raised);
    color: var(--ink-primary);
    font-family: var(--font-ui);
    font-size: 0.95em;
    text-align: left;
    cursor: pointer;
  }
  .app-row:hover {
    border-color: var(--ink-muted);
    background: var(--surface-overlay);
  }
  .app-name {
    flex: 1 1 auto;
    font-weight: 600;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .app-ver {
    flex: 0 0 auto;
    color: var(--ink-faint);
    font-family: var(--font-mono);
    font-size: 0.8em;
  }
  .app-size {
    flex: 0 0 auto;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    color: var(--ink-muted);
  }
  .loading {
    padding: var(--sp-3) 0 0;
    color: var(--ink-muted);
    font-size: 0.9em;
  }
  .skeletons {
    list-style: none;
    margin: var(--sp-2) 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .skeleton-row {
    height: 2.6em;
    border-radius: var(--radius);
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
    .skeleton-row {
      animation: none;
    }
  }
  .note {
    margin: var(--sp-2) 0;
    padding: var(--sp-2) var(--sp-3);
    border: 1px dashed var(--border-subtle);
    border-radius: var(--radius);
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .empty {
    padding: var(--sp-6) 0;
    text-align: center;
    color: var(--ink-muted);
  }
  .error {
    margin: var(--sp-2) 0 0;
    color: var(--state-danger);
    font-size: 0.85em;
  }
  .scan-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
  }
  .prog-text {
    color: var(--ink-muted);
    font-size: 0.85em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .prog-text.mono {
    flex: 1 1 auto;
    font-family: var(--font-mono);
  }
  .btns {
    display: flex;
    justify-content: flex-end;
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
    font-weight: 600;
  }
  .delete {
    font-variant-numeric: tabular-nums;
  }
</style>
