<script lang="ts">
  /**
   * 「开发清理」路由（move 7 第一段 / plan 020）。以 Clean.svelte 为蓝本复用其
   * 稳定三区 + rAF 批处理 + 删除信任链，唯一实质差异是**用户选定的目标目录**（F1）：
   * 默认 `~/`（与 CLI `mc purge` 一致，R3），可经原生目录选择器改选（R4）。
   * 不自动开扫——进入即 idle 态（R2：选目录是本路由的「首屏问题」，与 Clean 的自动答不同）。
   */
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    scanPurge,
    purge,
    cancelScan,
    openTrash,
    undo,
    userHome,
    openFdaSettings,
    type CleanReport,
    type RestoreReport,
    type ScanResult,
  } from "../lib/ipc";
  import {
    upsertFound,
    computeSegments,
    aggregateByCategory,
    formatBytes,
    shellQuote,
    type LiveItem,
    type FoundData,
  } from "../lib/format";
  import { withViewTransition } from "../lib/transition";
  import { nextToast, dismissToast, type ToastState } from "../lib/toast";
  import { KNOWN_PURGE_CATEGORIES } from "../lib/categories";
  import Shell from "../lib/Shell.svelte";
  import SummaryHeader from "../lib/SummaryHeader.svelte";
  import StreamingList from "../lib/StreamingList.svelte";
  import CleanReceipt from "../lib/CleanReceipt.svelte";
  import UndoToast from "../lib/UndoToast.svelte";
  import ConfirmDelete from "../lib/ConfirmDelete.svelte";
  import type { ConfirmItem } from "../lib/ConfirmDelete.svelte";
  import type { Command } from "../lib/palette";
  import { registerRouteCommands } from "../lib/palette-registry.svelte";

  type Phase = "idle" | "scanning" | "results" | "cleaning" | "done";

  let phase = $state<Phase>("idle");
  let target = $state(""); // 当前目标目录（绝对路径，R3/R5：始终合法可展示）
  let items = $state<LiveItem[]>([]);
  let error = $state<string | null>(null);
  let skipped = $state<string[]>([]);
  let showSkipped = $state(false);
  let scanningPath = $state(""); // purge 无 RuleProgress，以当前遍历路径示进度（R16）

  let confirmItems = $state<ConfirmItem[] | null>(null);
  let cleaningPath = $state("");
  let lastReport = $state<CleanReport | null>(null);
  let lastRunId = $state<string | null>(null);
  let toast = $state<ToastState>(null);

  const scanning = $derived(phase === "scanning");
  const cats = $derived(
    aggregateByCategory(items, KNOWN_PURGE_CATEGORIES, phase !== "scanning"),
  );
  const selectedItems = $derived(items.filter((i) => i.selected));
  const selectedSize = $derived(
    items.reduce((s, i) => (i.selected ? s + i.size : s), 0),
  );
  const segments = $derived(
    computeSegments(cats.map((c) => ({ ...c, size: c.selectedSize }))),
  );

  // ---- Cmd+K 命令面板路由动作命令（U2）。**严格镜像按钮的相位可用性**（KTD2 / 评审 correctness+adversarial）；
  // run 引既有函数保留删除分流（KTD3）。选目录在扫描/清理中省略（对应按钮此时 disabled）；未选目标不出扫描命令；
  // 移入废纸篓仅 results 相位（对应按钮仅此相位渲染）——否则扫描期 Safe 预选会让删除命令在途扫描时并发触发。----
  const paletteCommands = $derived<Command[]>([
    ...(phase !== "scanning" && phase !== "cleaning"
      ? [{ id: "purge.chooseDir", title: "选择目录", keywords: ["dir", "choose", "mulu", "xuanze"], run: chooseDir }]
      : []),
    ...(phase === "scanning"
      ? [{ id: "purge.cancel", title: "取消扫描", keywords: ["cancel", "quxiao"], run: cancel }]
      : []),
    ...(phase !== "scanning" && phase !== "cleaning" && target
      ? [{ id: "purge.scan", title: phase === "idle" ? "开始扫描" : "重新扫描", keywords: ["scan", "saomiao"], run: startScan }]
      : []),
    ...(phase === "results" && selectedItems.length > 0
      ? [{ id: "purge.trash", title: "移入废纸篓", keywords: ["trash", "delete", "feizhilou"], run: primaryDelete }]
      : []),
  ]);
  registerRouteCommands(() => paletteCommands);

  function setPhase(p: Phase) {
    withViewTransition(() => {
      phase = p;
    });
  }

  // 清空扫描产物回 idle（换目标 / 扫描被取消或失败时的统一收敛点）。
  function resetToIdle() {
    items = [];
    index.clear();
    buffer = [];
    skipped = [];
    lastReport = null;
    setPhase("idle");
  }

  // ---- 目录选择（F1 / R4 / R5）----
  async function chooseDir() {
    try {
      const picked = await open({ directory: true, defaultPath: target || undefined });
      // 取消返回 null → 保留原目标；只接受字符串（目录路径）。
      if (typeof picked === "string" && picked.length > 0 && picked !== target) {
        target = picked;
        // 换目标即作废旧结果（评审 R1）：results/done 的 items 属旧目录，若保留会出现
        // 「目标标签是新目录、删除的却是旧目录项」的误导删除——立即清空回 idle。
        if (phase === "results" || phase === "done") {
          error = null;
          resetToIdle();
        }
      }
    } catch (e) {
      // 选择器失败不进入错误态（R5）：目标保持原值，仅控制台留痕。
      console.warn("目录选择失败：", e);
    }
  }

  // ---- 扫描：rAF 批处理流式 Found（复用 Clean 的 KTD2 模式）----
  let rafId = 0;
  let buffer: FoundData[] = [];
  const index = new Map<string, number>();

  function flush() {
    rafId = 0;
    if (phase !== "scanning") {
      buffer = [];
      return;
    }
    for (const f of buffer) upsertFound(items, index, f);
    buffer = [];
  }
  function scheduleFlush() {
    if (rafId === 0) rafId = requestAnimationFrame(flush);
  }

  async function startScan() {
    if (!target) return;
    if (rafId) cancelAnimationFrame(rafId);
    rafId = 0;
    buffer = [];
    index.clear();
    items = [];
    skipped = [];
    error = null;
    scanningPath = "";
    setPhase("scanning");
    let result: ScanResult | null = null;
    try {
      result = await scanPurge(target, (e) => {
        if (typeof e === "string") return; // "Complete"
        if ("Found" in e) {
          buffer.push(e.Found);
          scheduleFlush();
        } else if ("Scanning" in e) {
          scanningPath = e.Scanning.path;
        } else if ("SkippedNoPermission" in e) {
          skipped.push(e.SkippedNoPermission.path);
        } else if ("Error" in e) {
          error = e.Error;
        }
      });
    } catch (err) {
      // 取消也走这里；reject 后部分项会被清空回 idle，故无论已流入多少项都要留横幅说明原因。
      if (error === null) error = String(err);
    }
    if (rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    if (result) {
      // 以 resolved ScanResult 为权威终值（消除流式/终态漂移，同 Clean KTD5）。
      items = result.categories.flatMap((g) =>
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
      index.clear();
      buffer = [];
      setPhase("results");
    } else {
      // 扫描被取消或命令失败（评审 R2）：后端此时不写 last_purge，若保留流式部分项
      // 会形成「可见但删除必然落空」的假结果——清空回 idle，错误横幅仍呈现原因。
      resetToIdle();
    }
  }

  function cancel() {
    void cancelScan();
  }

  function toggle(item: LiveItem) {
    item.selected = !item.selected;
  }

  // ---- 删除：纯 Safe/Moderate 直删；含 Risky 走 type-to-confirm（AE4/AE7）----
  function primaryDelete() {
    if (selectedItems.length === 0) return;
    if (selectedItems.some((i) => i.safety === "Risky")) {
      confirmItems = selectedItems.map((i) => ({
        path: i.path,
        size: i.size,
        safety: i.safety,
      }));
      return;
    }
    void doPurge(
      selectedItems.map((i) => i.path),
      "",
    );
  }

  async function doPurge(paths: string[], token: string) {
    confirmItems = null;
    if (paths.length === 0) return;
    error = null;
    cleaningPath = "";
    setPhase("cleaning");
    let report: CleanReport | null = null;
    let runId: string | null = null;
    try {
      const resp = await purge(paths, token, (e) => {
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
      undoResult = null; // 新一次清理：清掉上次撤销缓存（评审 #1）。
      if (report.success_count > 0) {
        toast = nextToast(toast, report.success_count, report.total_freed);
      }
    }
    setPhase("done");
  }

  function restoreInFinder() {
    void openTrash();
  }

  // 真一键撤销：仅当本次清理写出账本条目（run_id 非空）时可用，按 run_id 精确命中（KTD1）。
  //
  // **撤销至多发一次 IPC**（评审 #1，同 Clean.svelte）：回执与吐司共享 undoAction，各自组件内的
  // in-flight 守卫互不可见——先点吐司再点回执会二次 restore 得到全跳过的误导报告。故上提撤销生命周期
  // 到父组件：`undoPromise` 合并并发调用，`undoResult` 缓存有实际放回的结果供后续入口重放、不再发
  // IPC；空报告不缓存，允许各入口走 Finder 降级重试（R4）。
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

  // toast 自动消失（6s），同 Clean。
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
    // 默认目标 = 用户主目录（R3，与 CLI `mc purge` 默认一致）。失败不阻断——
    // 目标留空时「开始扫描」禁用，用户仍可经选择器指定目录（R5）。
    void userHome()
      .then((home) => {
        if (!target) target = home;
      })
      .catch((e) => console.warn("获取主目录失败：", e));
    return () => {
      if (rafId) cancelAnimationFrame(rafId);
      // 仅在确有在途操作时协作取消（AE6 / 评审 R3）：Purge 常驻 idle，无条件 cancel 会与
      // 下一 tab（Clean）mount 自动扫描的 begin_operation 竞速，可能误杀刚起步的新扫描。
      if (phase === "scanning" || phase === "cleaning") void cancelScan();
    };
  });
</script>

<Shell>
  {#snippet summary()}
    {#if phase === "done" && lastReport}
      <CleanReceipt
        report={lastReport}
        onRestore={restoreInFinder}
        onUndo={undoAction}
        {onUndone}
      />
    {:else}
      <SummaryHeader amount={selectedSize} {segments} {scanning} />
      {#if error && !scanning}<p class="error" role="alert">出错：{error}</p>{/if}
    {/if}
  {/snippet}

  {#snippet list()}
    {#if phase !== "done"}
      <!-- 目标目录横条：全程可见（R3），扫描/删除中禁改 -->
      <div class="target-bar">
        <span class="target-label">目标目录</span>
        <code class="target-path" title={target}>{target || "获取中…"}</code>
        <button
          class="choose"
          onclick={chooseDir}
          disabled={phase === "scanning" || phase === "cleaning"}
        >
          选择目录
        </button>
      </div>
      {#if phase === "idle"}
        <p class="idle-hint">
          扫描该目录下的开发产物——node_modules、Rust target、Xcode DerivedData、
          Docker/brew 缓存等，按项目根标记精确识别，默认只预选可安全重建的项。
        </p>
      {:else}
        <StreamingList
          {items}
          knownOrder={KNOWN_PURGE_CATEGORIES}
          {scanning}
          onToggle={toggle}
          cliCommand={`mc purge ${shellQuote(target)}`}
          cliNote="清理该目录下全部可安全释放项"
        />
        {#if phase === "results" && items.length === 0 && !error}
          <p class="empty">未发现开发产物——该目录很干净。</p>
        {/if}
        {#if phase === "results" && skipped.length > 0}
          <div class="skipped">
            <button class="link" onclick={() => (showSkipped = !showSkipped)}>
              因权限跳过 {skipped.length} 项 {showSkipped ? "收起" : "展开"}
            </button>
            {#if showSkipped}
              <ul class="skipped-list">
                {#each skipped as p (p)}
                  <li title={p}>{p}</li>
                {/each}
              </ul>
            {/if}
            <div class="skipped-guide">
              <button class="link" onclick={() => void openFdaSettings()}>
                打开磁盘访问权限设置
              </button>
              <p class="skipped-hint">授权后需完全退出并重启 macCleaner 才生效。</p>
            </div>
          </div>
        {/if}
      {/if}
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if phase === "scanning"}
      <div class="scan-actions">
        <div class="prog" aria-live="polite">
          <span class="prog-text mono" title={scanningPath}>
            {scanningPath ? `扫描中 · ${scanningPath}` : "扫描中…"}
          </span>
        </div>
        <button class="ghost-danger" onclick={cancel}>取消</button>
      </div>
    {:else if phase === "cleaning"}
      <div class="scan-actions">
        <span class="prog-text mono" title={cleaningPath}>
          {cleaningPath || "移入废纸篓中…"}
        </span>
      </div>
    {:else if phase === "done"}
      <div class="btns">
        <button class="primary" onclick={startScan}>再次扫描</button>
      </div>
    {:else if phase === "idle"}
      <div class="btns">
        <button class="primary" disabled={!target} onclick={startScan}>开始扫描</button>
      </div>
    {:else}
      <!-- results -->
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
    onConfirm={(token) => doPurge(confirmItems?.map((i) => i.path) ?? [], token)}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
  .target-bar {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) 0;
  }
  .target-label {
    flex: 0 0 auto;
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .target-path {
    flex: 1 1 auto;
    min-width: 0;
    font-family: var(--font-mono);
    font-size: 0.85em;
    color: var(--ink-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .choose {
    flex: 0 0 auto;
    padding: var(--sp-1) var(--sp-3);
  }
  .idle-hint {
    margin: var(--sp-3) 0 0;
    color: var(--ink-muted);
    font-size: 0.9em;
    line-height: 1.6;
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
  .prog {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-width: 0;
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
  .ghost-danger {
    flex: 0 0 auto;
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
    font-family: var(--font-ui);
    font-size: 0.85em;
  }
  .skipped {
    padding: var(--sp-3) 0 0;
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
  .skipped-guide {
    margin: var(--sp-2) 0 0;
  }
  .skipped-hint {
    margin: var(--sp-1) 0 0;
    font-family: var(--font-ui);
    font-size: 0.8em;
    color: var(--ink-muted);
  }
</style>
