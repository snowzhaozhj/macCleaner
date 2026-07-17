<script lang="ts">
  /**
   * 清理回执（U7 / R15 R16；GUI 一键 undo 增强）。数据源 = `CleanReport`（后端权威）。
   * 排版化呈现：释放量 + 成功计数 + 恢复入口；失败项优雅分列（继承逐项优雅降级语义）。
   * 刻意**无 confetti、无大绿勾 hero**（不庆祝、不表演）。
   *
   * **恢复行为按路由注入**（KTD5）：
   * - clean/purge 传 `onUndo`（真一键确定性放回）→ 呈现「撤销清理」；点后渲染 RestoreReport
   *   三态（放回/跳过/失败），跳过与失败视觉分列；成功后按钮转已恢复态（防二次点击重跑 restore
   *   产生「全失败」报告，R7）；无落点/报错 → 退回 Finder 手动放回（R4）。
   * - uninstall 不传 `onUndo` → 只呈现既有「在访达中恢复」（开 Finder），行为不变。
   */
  import { formatBytes, summarizeReport } from "./format";
  import { countRestore, type CleanReport, type RestoreReport, type RestoreStatus } from "./ipc";
  import PathText from "./PathText.svelte";

  let {
    report,
    onRestore,
    onUndo = null,
    onUndone = null,
  }: {
    report: CleanReport;
    /** Finder 手动放回（打开废纸篓）——所有路由都有，作真 undo 的降级与 uninstall 的唯一路径。 */
    onRestore: () => void;
    /** 真一键撤销：按 run_id 确定性放回，返回 RestoreReport。为 null 时不呈现「撤销清理」。 */
    onUndo?: (() => Promise<RestoreReport>) | null;
    /** 撤销成功（放回 > 0）后回调，供父组件收起对应 UndoToast，避免「已移到废纸篓」文案与事实矛盾。 */
    onUndone?: (() => void) | null;
  } = $props();

  const r = $derived(summarizeReport(report));

  type UndoState = "idle" | "running" | "done" | "empty" | "error";
  let undoState = $state<UndoState>("idle");
  let restoreResult = $state<RestoreReport | null>(null);
  let undoError = $state<string | null>(null);

  const counts = $derived(restoreResult ? countRestore(restoreResult) : null);
  const failedOutcomes = $derived(
    restoreResult?.outcomes.filter((o) => o.status === "failed") ?? [],
  );
  const skippedOutcomes = $derived(
    restoreResult?.outcomes.filter(
      (o) => o.status === "skipped_target_occupied" || o.status === "skipped_trash_missing",
    ) ?? [],
  );

  async function handleUndo() {
    if (!onUndo || undoState === "running" || undoState === "done") return;
    undoState = "running";
    undoError = null;
    try {
      const result = await onUndo();
      if (result.outcomes.length === 0) {
        // 命中无落点 / run_id 不存在 → 无可确定性放回，退回 Finder 手动路径（R4）。
        undoState = "empty";
      } else {
        restoreResult = result;
        undoState = "done";
        onUndone?.();
      }
    } catch (e) {
      undoError = String(e);
      undoState = "error";
    }
  }

  function skipLabel(status: RestoreStatus): string {
    return status === "skipped_target_occupied"
      ? "原位置已被占用（原文件未受影响）"
      : "废纸篓中已无此项";
  }
</script>

<div class="receipt">
  <p class="line">
    已释放 <strong class="freed">{formatBytes(r.freed)}</strong>
    <span class="muted">· {r.successCount} 项已移到废纸篓</span>
  </p>

  {#if onUndo}
    <!-- clean/purge：真一键撤销。撤销进行中/成功后禁用，防重复触发（R7）。 -->
    {#if undoState === "idle" || undoState === "running"}
      <p class="restore-hint">
        <button
          class="link undo"
          onclick={handleUndo}
          disabled={undoState === "running"}
        >
          {undoState === "running" ? "撤销中…" : "撤销清理（放回原处）"}
        </button>
      </p>
    {/if}

    <!-- 撤销结果就地播报（屏幕阅读器可闻，R3）。 -->
    <div class="undo-result" role="status" aria-live="polite">
      {#if undoState === "done" && counts}
        <p class="line">
          已放回 <strong class="restored">{counts.restored}</strong> 项
          {#if counts.skipped > 0 || counts.failed > 0}
            <span class="muted">
              · 跳过 {counts.skipped} · 失败 {counts.failed}
            </span>
          {/if}
        </p>
        {#if skippedOutcomes.length > 0}
          <section class="skips">
            <p class="sec-head">以下项已跳过（原文件未受影响）：</p>
            <ul>
              {#each skippedOutcomes as o (o.original)}
                <li>
                  <PathText path={o.original} />
                  <span class="sec-reason">{skipLabel(o.status)}</span>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
        {#if failedOutcomes.length > 0}
          <section class="failures">
            <p class="fail-head">{failedOutcomes.length} 项放回失败：</p>
            <ul>
              {#each failedOutcomes as o (o.original)}
                <li>
                  <PathText path={o.original} />
                  {#if o.error}<span class="fail-reason">{o.error}</span>{/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {:else if undoState === "empty"}
        <p class="restore-hint">
          本次清理无法自动放回。<button class="link" onclick={onRestore}>在访达中恢复</button>
        </p>
      {:else if undoState === "error"}
        <p class="restore-hint error">
          撤销失败{undoError ? `：${undoError}` : ""}。<button class="link" onclick={onRestore}>在访达中恢复</button>
        </p>
      {/if}
    </div>
  {:else}
    <!-- uninstall/analyze：仅 Finder 手动放回，行为不变。 -->
    <p class="restore-hint">
      文件在废纸篓中可恢复。<button class="link" onclick={onRestore}>在访达中恢复</button>
    </p>
  {/if}

  {#if r.failureCount > 0}
    <section class="failures">
      <p class="fail-head">{r.failureCount} 项未能移除：</p>
      <ul>
        {#each r.failed as f (f.path)}
          <li>
            <PathText path={f.path} />
            {#if f.error}<span class="fail-reason">{f.error}</span>{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .receipt {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .line {
    margin: 0;
    font-size: 1.05rem;
    color: var(--ink-primary);
  }
  /* 释放量/已放回数用中性成功色的等宽数字，非填色 hero；不喧宾（R18） */
  .freed,
  .restored {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    color: var(--state-success);
  }
  .muted {
    color: var(--ink-muted);
    font-size: 0.9rem;
  }
  .undo-result {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .restore-hint {
    margin: 0;
    color: var(--ink-muted);
    font-size: 0.9em;
  }
  .restore-hint.error {
    color: var(--state-danger);
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    cursor: pointer;
    font-family: var(--font-ui);
    font-size: 1em;
  }
  .link:hover:not(:disabled) {
    text-decoration: underline;
  }
  .link:disabled {
    color: var(--ink-faint);
    cursor: not-allowed;
  }
  /* 跳过：中性提示，与失败的 danger 语义分列（R3）——跳过表示原文件未受影响。 */
  .skips,
  .failures {
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }
  .sec-head,
  .fail-head {
    margin: 0 0 var(--sp-2);
    color: var(--ink-muted);
    font-size: 0.9em;
  }
  .skips ul,
  .failures ul {
    list-style: none;
    margin: 0;
    padding: 0;
    max-height: 160px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .skips li,
  .failures li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    min-width: 0;
  }
  .sec-reason {
    flex: 0 0 auto;
    font-size: 0.8em;
    color: var(--ink-muted);
  }
  .fail-reason {
    flex: 0 0 auto;
    font-size: 0.8em;
    color: var(--ink-faint);
  }
</style>
