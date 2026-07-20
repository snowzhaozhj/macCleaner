<script lang="ts">
  /**
   * 「孤儿残留」路由（plan 028 / 反向卸载 GUI 入口）。**单阶段**：进入即全局扫描 `~/Library`
   * 找父 App 已不存在的孤儿残留 → 审查（**一律未勾选**）→ 勾选 → 移废纸篓 → 回执 + 一键撤销。
   * 与「卸载」（正向：选一个已装 App 卸载它的残留）互补，镜像 CLI `mc orphans`。
   *
   * 扫描端同步非流式（KTD1，同 scan_uninstall）：核心一次性返回全量快照，无进度/取消。删除端
   * 复用 clean/purge 的 run_id 一键撤销信任链。残留一律 preselect=false 由核心保证（KTD2）——
   * 工具主动发现、非用户点名，故列表默认全未勾，须用户显式勾选要回收的项。
   */
  import { onMount } from "svelte";
  import {
    scanOrphans,
    cleanOrphans,
    cancelScan,
    openTrash,
    undo,
    type CleanReport,
    type RestoreReport,
  } from "../lib/ipc";
  import { aggregateByCategory, computeSegments, formatBytes, type LiveItem } from "../lib/format";
  import { withViewTransition } from "../lib/transition";
  import { nextToast, dismissToast, type ToastState } from "../lib/toast";
  import Shell from "../lib/Shell.svelte";
  import SummaryHeader from "../lib/SummaryHeader.svelte";
  import StreamingList from "../lib/StreamingList.svelte";
  import CleanReceipt from "../lib/CleanReceipt.svelte";
  import UndoToast from "../lib/UndoToast.svelte";
  import ConfirmDelete from "../lib/ConfirmDelete.svelte";
  import type { ConfirmItem } from "../lib/ConfirmDelete.svelte";
  import type { Command } from "../lib/palette";
  import { registerRouteCommands } from "../lib/palette-registry.svelte";

  // 单阶段相位机（KTD1，较 uninstall 九态大幅简化）：进入即扫，无中途取消的流式扫描。
  type Phase = "loading" | "ready" | "empty" | "error" | "deleting" | "done";

  let phase = $state<Phase>("loading");
  let items = $state<LiveItem[]>([]);
  let error = $state<string | null>(null);

  let confirmItems = $state<ConfirmItem[] | null>(null);
  let cleaningPath = $state("");
  let lastReport = $state<CleanReport | null>(null);
  let lastRunId = $state<string | null>(null);
  let toast = $state<ToastState>(null);

  // 孤儿分类是动态的（各项自带 category，如「应用残留 (Caches)」）——knownOrder=[] 不锁序，
  // 完成态聚合（同 uninstall 审查）。
  const cats = $derived(aggregateByCategory(items, [], true));
  const selectedItems = $derived(items.filter((i) => i.selected));
  const selectedSize = $derived(
    items.reduce((s, i) => (i.selected ? s + i.size : s), 0),
  );
  const segments = $derived(
    computeSegments(cats.map((c) => ({ ...c, size: c.selectedSize }))),
  );
  const reviewing = $derived(phase === "ready" || phase === "deleting");

  function setPhase(p: Phase) {
    withViewTransition(() => {
      phase = p;
    });
  }

  // ---- 扫描：同步一次性（无流式 Found、无进度、无取消，KTD1）----
  async function startScan() {
    error = null;
    items = [];
    setPhase("loading");
    try {
      const result = await scanOrphans();
      items = result.categories.flatMap((g) =>
        g.items.map((it) => ({
          path: it.path,
          size: it.size,
          safety: it.safety,
          category: it.category,
          impact: it.impact,
          recovery: it.recovery,
          selected: it.selected, // 核心保证全 false（KTD2）——不在此重设，语义单一来源在核心。
        })),
      );
      setPhase(items.length > 0 ? "ready" : "empty");
    } catch (err) {
      error = String(err);
      setPhase("error");
    }
  }

  function toggle(item: LiveItem) {
    item.selected = !item.selected;
  }

  // ---- 删除：纯 Safe/Moderate 直删；含 Risky 走 type-to-confirm（核心保证孤儿不产 Risky，
  // 此分流是纵深防御，与 clean/purge 一致，KTD5）----
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
    void doClean(
      selectedItems.map((i) => i.path),
      "",
    );
  }

  async function doClean(paths: string[], token: string) {
    confirmItems = null;
    if (paths.length === 0) return;
    error = null;
    cleaningPath = "";
    // 清旧回执：否则上次成功的 report 会在本次删除 reject 时残留、done 相位误当本次回执（同 uninstall）。
    lastReport = null;
    setPhase("deleting");
    let report: CleanReport | null = null;
    let runId: string | null = null;
    try {
      const resp = await cleanOrphans(paths, token, (e) => {
        if (typeof e === "string") return;
        if ("CleaningFile" in e) cleaningPath = e.CleaningFile.path;
        else if ("Error" in e) error = e.Error;
      });
      report = resp.report;
      runId = resp.run_id;
    } catch (err) {
      error = String(err);
    }
    if (report) {
      lastReport = report;
      lastRunId = runId;
      undoResult = null; // 新一次删除：清掉上次撤销缓存，使本次撤销重新可发 IPC。
      if (report.success_count > 0) {
        toast = nextToast(toast, report.success_count, report.total_freed);
      }
    }
    setPhase("done");
  }

  function restoreInFinder() {
    void openTrash();
  }

  // 真一键撤销：仅当本次删除写出账本条目（run_id 非空）时可用（同 Clean/Purge，KTD4）。
  // 至多发一次 IPC——生命周期上提到本组件，undoPromise 合并并发、undoResult 缓存有实际放回的结果。
  let undoResult: RestoreReport | null = null;
  let undoPromise: Promise<RestoreReport> | null = null;

  function runUndo(): Promise<RestoreReport> {
    const id = lastRunId;
    if (!id) return Promise.resolve({ outcomes: [], dry_run: false });
    if (undoResult) return Promise.resolve(undoResult);
    undoPromise ??= undo(id)
      .then((r) => {
        if (r.outcomes.length > 0) undoResult = r;
        return r;
      })
      .finally(() => {
        undoPromise = null;
      });
    return undoPromise;
  }

  const undoAction = $derived<(() => Promise<RestoreReport>) | null>(
    lastRunId ? runUndo : null,
  );

  function onUndone() {
    toast = dismissToast();
  }

  // toast 自动消失（6s），同 Clean/Purge/Uninstall。
  $effect(() => {
    const t = toast;
    if (!t) return;
    const seq = t.seq;
    const timer = setTimeout(() => {
      if (toast?.seq === seq) toast = dismissToast();
    }, 6000);
    return () => clearTimeout(timer);
  });

  // ---- Cmd+K 路由动作命令：镜像按钮相位可用性——ready/done 可重扫、有选中项可删（同 Clean）。----
  const paletteCommands = $derived<Command[]>([
    ...(phase === "ready" || phase === "empty" || phase === "error" || phase === "done"
      ? [{ id: "orphans.scan", title: phase === "done" ? "再次扫描" : "重新扫描孤儿", keywords: ["scan", "rescan", "orphans", "guer"], run: startScan }]
      : []),
    ...(phase === "ready" && selectedItems.length > 0
      ? [{ id: "orphans.trash", title: "移入废纸篓", keywords: ["trash", "delete", "feizhilou"], run: primaryDelete }]
      : []),
  ]);
  registerRouteCommands(() => paletteCommands);

  onMount(() => {
    void startScan();
    return () => {
      // 仅删除在途时协作取消（clean_orphans 走 begin_operation）；扫描是纯查询无取消 flag，
      // 无条件 cancel 会与下一 tab mount 的 begin_operation 竞速（同 Uninstall）。
      if (phase === "deleting") void cancelScan();
    };
  });
</script>

<Shell>
  {#snippet summary()}
    {#if phase === "done"}
      {#if lastReport}
        <CleanReceipt
          report={lastReport}
          onRestore={restoreInFinder}
          onUndo={undoAction}
          {onUndone}
        />
      {:else}
        <p class="error" role="alert">清理失败：{error ?? "未知错误"}</p>
      {/if}
    {:else}
      <SummaryHeader amount={selectedSize} {segments} scanning={phase === "loading"} />
      {#if reviewing}
        <p class="note">
          孤儿残留需<strong>手动勾选</strong>要回收的项——已卸载应用的数据可能仍需保留，故默认不选。
        </p>
      {/if}
      {#if error && phase !== "deleting"}<p class="error" role="alert">出错：{error}</p>{/if}
    {/if}
  {/snippet}

  {#snippet list()}
    {#if phase === "loading"}
      <p class="loading" role="status" aria-live="polite">正在扫描 ~/Library 孤儿残留…</p>
      <ul class="skeletons" aria-hidden="true">
        {#each [0, 1, 2, 3] as i (i)}
          <li class="skeleton-row"></li>
        {/each}
      </ul>
    {:else if phase === "empty"}
      <p class="empty">未发现孤儿残留。</p>
    {:else if phase === "ready" || phase === "deleting"}
      <StreamingList
        {items}
        scanning={false}
        onToggle={toggle}
        cliCommand="mc orphans"
        cliNote="反向卸载：清理父 App 已不存在的孤儿残留"
      />
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if phase === "loading"}
      <div class="scan-actions">
        <span class="prog-text">扫描中…</span>
      </div>
    {:else if phase === "deleting"}
      <div class="scan-actions">
        <span class="prog-text mono" title={cleaningPath}>
          {cleaningPath || "移入废纸篓中…"}
        </span>
      </div>
    {:else if phase === "done"}
      <div class="btns">
        <button class="primary" onclick={startScan}>再次扫描</button>
      </div>
    {:else if phase === "ready"}
      <div class="btns">
        <button onclick={startScan}>重新扫描</button>
        <button
          class="primary delete"
          disabled={selectedItems.length === 0}
          onclick={primaryDelete}
        >
          移入废纸篓 · 释放 {formatBytes(selectedSize)}
        </button>
      </div>
    {:else}
      <!-- empty / error -->
      <div class="btns">
        <button onclick={startScan}>重新扫描</button>
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
      onUndo={undoAction}
      onDismiss={() => (toast = dismissToast())}
    />
  {/key}
{/if}

{#if confirmItems}
  <ConfirmDelete
    items={confirmItems}
    onConfirm={(token) => doClean(confirmItems?.map((i) => i.path) ?? [], token)}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .note {
    margin: var(--sp-2) 0 0;
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .empty {
    padding: var(--sp-6) 0;
    text-align: center;
    color: var(--ink-muted);
  }
  .loading {
    color: var(--ink-muted);
    font-size: 0.9em;
  }
  .error {
    margin: var(--sp-2) 0 0;
    color: var(--state-danger);
    font-size: 0.85em;
  }
  .skeletons {
    list-style: none;
    margin: var(--sp-3) 0 0;
    padding: 0;
  }
  .skeleton-row {
    height: 44px;
    margin-bottom: var(--sp-2);
    border-radius: var(--radius);
    background: linear-gradient(
      90deg,
      var(--surface-raised) 25%,
      color-mix(in oklch, var(--surface-raised) 60%, var(--ink-faint)) 50%,
      var(--surface-raised) 75%
    );
    background-size: 200% 100%;
    animation: shimmer 1.4s ease-in-out infinite;
  }
  @keyframes shimmer {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
  }
  /* 尊重减少动态偏好——与 Uninstall/Analyze 骨架一致（a11y）。 */
  @media (prefers-reduced-motion: reduce) {
    .skeleton-row {
      animation: none;
    }
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
