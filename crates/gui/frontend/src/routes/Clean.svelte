<script lang="ts">
  import { onMount } from "svelte";
  import {
    scanClean,
    clean,
    cancelScan,
    openTrash,
    undo,
    type CleanReport,
    type RestoreReport,
    type ScanResult,
  } from "../lib/ipc";
  import {
    upsertFound,
    aggregateByCategory,
    computeSegments,
    formatBytes,
    type LiveItem,
    type FoundData,
  } from "../lib/format";
  import { withViewTransition } from "../lib/transition";
  import { nextToast, dismissToast, type ToastState } from "../lib/toast";
  import { KNOWN_CATEGORIES } from "../lib/categories";
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
  let items = $state<LiveItem[]>([]);
  let error = $state<string | null>(null);
  let skipped = $state<string[]>([]);
  let showSkipped = $state(false);
  let scanProg = $state<{ current: number; total: number } | null>(null);

  let confirmItems = $state<ConfirmItem[] | null>(null);
  let cleaningPath = $state("");
  let lastReport = $state<CleanReport | null>(null);
  let lastRunId = $state<string | null>(null);
  let toast = $state<ToastState>(null);

  const scanning = $derived(phase === "scanning");
  const cats = $derived(
    aggregateByCategory(items, KNOWN_CATEGORIES, phase !== "scanning"),
  );
  const selectedItems = $derived(items.filter((i) => i.selected));
  const selectedSize = $derived(
    items.reduce((s, i) => (i.selected ? s + i.size : s), 0),
  );
  // 分段横条按**已选**占比呈现，使横条总量与首屏主数字/主按钮量恒等（R10）。
  const segments = $derived(
    computeSegments(cats.map((c) => ({ ...c, size: c.selectedSize }))),
  );

  function setPhase(p: Phase) {
    withViewTransition(() => {
      phase = p;
    });
  }

  // ---- 扫描：rAF 批处理流式 Found（KTD2）----
  let rafId = 0;
  let buffer: FoundData[] = [];
  const index = new Map<string, number>();

  function flush() {
    rafId = 0;
    // 只在扫描期消费缓冲：resolve 后 items 已被权威 ScanResult 重建、index 失效，
    // 迟到的 Found 若再走 upsertFound 会按空 index 误建重复项/越界（correctness review P3）。
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
    if (rafId) cancelAnimationFrame(rafId);
    rafId = 0;
    buffer = [];
    index.clear();
    items = [];
    skipped = [];
    error = null;
    scanProg = null;
    setPhase("scanning");
    let result: ScanResult | null = null;
    try {
      result = await scanClean((e) => {
        if (typeof e === "string") return; // "Complete"
        if ("Found" in e) {
          buffer.push(e.Found);
          scheduleFlush();
        } else if ("RuleProgress" in e) {
          scanProg = { current: e.RuleProgress.current, total: e.RuleProgress.total };
        } else if ("SkippedNoPermission" in e) {
          skipped.push(e.SkippedNoPermission.path);
        } else if ("Error" in e) {
          error = e.Error;
        }
      });
    } catch (err) {
      // 取消也走这里
      if (error === null && items.length === 0) error = String(err);
    }
    // 落尾帧 + 以 resolved ScanResult 为权威终值（KTD5：消除流式/终态漂移）。
    if (rafId) {
      cancelAnimationFrame(rafId);
      rafId = 0;
    }
    if (result) {
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
      // items 已权威重建，流式索引/缓冲随之失效——清空避免迟到 Found 误用旧索引（P3）。
      index.clear();
      buffer = [];
    } else {
      flush();
    }
    setPhase("results");
  }

  function cancel() {
    void cancelScan();
  }

  function toggle(item: LiveItem) {
    item.selected = !item.selected;
  }

  // ---- 删除：全 Safe 直删；含 Risky 才走 ConfirmDelete（v1 Clean 不触发，保留给未来）----
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
    setPhase("cleaning");
    let report: CleanReport | null = null;
    let runId: string | null = null;
    try {
      const resp = await clean(paths, token, (e) => {
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
      undoResult = null; // 新一次清理：清掉上次撤销缓存，使本次撤销重新可发 IPC。
      // 单实例 toast：诚实「已移到废纸篓」（R11/R13）。
      if (report.success_count > 0) {
        toast = nextToast(toast, report.success_count, report.total_freed);
      }
    }
    setPhase("done");
  }

  function restoreInFinder() {
    void openTrash();
  }

  // 真一键撤销：仅当本次清理写出了账本条目（run_id 非空）时可用；否则回执/toast 不呈现撤销、
  // 退回 Finder 手动放回（R2/R4）。按 run_id 精确命中，杜绝共享账本竞据劫持（KTD1）。
  //
  // **撤销至多发一次 IPC**（评审 #1）：回执与吐司两个入口绑定同一 undoAction，各自只守着自身组件内
  // 的 in-flight 状态、彼此看不见——先点吐司（成功即消失）再点回执仍 idle 的按钮，会对已放回的文件
  // 二次 restore，得到全 SkippedTrashMissing 的「已放回 0 项·跳过 N」误导报告，架空回执的 R7 护栏。
  // 故把撤销生命周期上提到父组件：`undoPromise` 合并并发调用；`undoResult` 缓存**有实际放回**的结果，
  // 后续任一入口再点都重放缓存、不再发 IPC——回执照样渲染真实「已放回 N」，二次 restore 不可能发生。
  // 空报告（无落点）不缓存：各入口据此走 Finder 降级、允许重试（R4），且重试仍空、不会误报。
  let undoResult: RestoreReport | null = null;
  let undoPromise: Promise<RestoreReport> | null = null;

  function runUndo(): Promise<RestoreReport> {
    const id = lastRunId;
    if (!id) return Promise.resolve({ outcomes: [], dry_run: false });
    if (undoResult) return Promise.resolve(undoResult); // 已成功放回：重放缓存，绝不二次 restore。
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

  // 撤销成功后收起 toast，避免「已移到废纸篓」文案与已撤销事实矛盾（R7）。
  function onUndone() {
    toast = dismissToast();
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

  // ---- Cmd+K 命令面板路由动作命令（U2）。**严格镜像按钮的相位可用性**（KTD2 / 评审 correctness+adversarial）：
  // 只在按钮可见的相位暴露对应命令——cleaning 相位不出扫描/删除命令、done 只出「再次扫描」。
  // 否则清理进行中经面板再触发 startScan/primaryDelete 会与在途操作并发（本仓反复防范，见下方 onMount 注释）。
  // run 引既有函数保留删除分流（KTD3）：primaryDelete 全 Safe 直删、含 Risky 走 ConfirmDelete，命令层不绕过。----
  const paletteCommands = $derived<Command[]>([
    ...(phase === "scanning"
      ? [{ id: "clean.cancel", title: "取消扫描", keywords: ["cancel", "quxiao"], run: cancel }]
      : []),
    ...(phase === "results" || phase === "done"
      ? [{ id: "clean.scan", title: phase === "done" ? "再次扫描" : "重新扫描", keywords: ["scan", "rescan", "chongxin"], run: startScan }]
      : []),
    ...(phase === "results" && selectedItems.length > 0
      ? [{ id: "clean.trash", title: "移入废纸篓", keywords: ["trash", "delete", "feizhilou"], run: primaryDelete }]
      : []),
  ]);
  registerRouteCommands(() => paletteCommands);

  onMount(() => {
    // FDA 已授权后进入 Clean 即给「首屏答案」——自动开扫，稳定外壳内内容填充（F1）。
    void startScan();
    return () => {
      if (rafId) cancelAnimationFrame(rafId);
      // 切走 tab 会销毁本组件——协作取消在途扫描，避免后台空扫 + 两次扫描并发写 last_scan
      // 致其被旧结果覆盖、后续 clean 静默漏删（correctness review P2）。cancelScan 幂等，安全。
      void cancelScan();
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
      <!-- 扫描期不在摘要区显示错误横幅：其高度变化会推动列表位移（防跳变）；错误在完成后呈现 -->
      {#if error && !scanning}<p class="error" role="alert">出错：{error}</p>{/if}
    {/if}
  {/snippet}

  {#snippet list()}
    {#if phase !== "done"}
      <StreamingList
        {items}
        knownOrder={KNOWN_CATEGORIES}
        {scanning}
        onToggle={toggle}
      />
      {#if phase === "results" && items.length === 0 && !error}
        <p class="empty">未发现可清理项——系统很干净。</p>
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
        </div>
      {/if}
    {/if}
  {/snippet}

  {#snippet actions()}
    {#if phase === "scanning"}
      <div class="scan-actions">
        <div class="prog" aria-live="polite">
          {#if scanProg}
            <span class="prog-text">扫描中 · {scanProg.current}/{scanProg.total}</span>
            <span class="prog-track" aria-hidden="true">
              <span
                class="prog-fill"
                style="transform: scaleX({scanProg.total > 0
                  ? scanProg.current / scanProg.total
                  : 0})"
              ></span>
            </span>
          {:else}
            <span class="prog-text">扫描中…</span>
          {/if}
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
    {:else}
      <!-- results / idle -->
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
    onConfirm={(token) => doClean(confirmItems?.map((i) => i.path) ?? [], token)}
    onCancel={() => (confirmItems = null)}
  />
{/if}

<style>
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
  /* 确定性进度条（RuleProgress 驱动）——取代 braille spinner（R6/R19） */
  .prog-track {
    flex: 1 1 auto;
    height: 4px;
    border-radius: 2px;
    background: var(--surface-raised);
    overflow: hidden;
  }
  .prog-fill {
    display: block;
    width: 100%;
    height: 100%;
    background: var(--accent);
    transform-origin: left;
    /* 用 transform 而非 width 做进度过渡：避免布局回流（impeccable 布局物理学） */
    transition: transform var(--dur-fast) var(--ease-out-quart);
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
  /* Clean 全 Safe：主删除按钮不染红（R18：红只跟随 Risky）；用 accent 强调即可 */
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
</style>
