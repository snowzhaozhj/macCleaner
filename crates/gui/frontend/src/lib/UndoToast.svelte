<script lang="ts">
  /**
   * 撤销 toast（U5 / R11 R13 / KTD4；GUI 一键 undo 增强）。
   *
   * 文案诚实为「已移到废纸篓」。**恢复行为按路由注入**（KTD5）：
   * - clean/purge 传 `onUndo` → 「撤销」做真一键确定性放回；成功后自动 dismiss（避免「已移到
   *   废纸篓」与已撤销事实矛盾），无落点/报错则退回 `onRestore`（开 Finder）。
   * - uninstall/analyze 不传 `onUndo` → 「在访达中恢复」维持开 Finder 原生「放回原处」，行为不变。
   * 单实例由父组件的 toast.ts 状态保证。
   */
  import { fly } from "svelte/transition";
  import { formatBytes } from "./format";
  import type { RestoreReport } from "./ipc";

  let {
    count,
    freed,
    onRestore,
    onDismiss,
    onUndo = null,
  }: {
    count: number;
    freed: number;
    onRestore: () => void;
    onDismiss: () => void;
    /** 真一键撤销；为 null 时「在访达中恢复」维持开 Finder 的既有行为。 */
    onUndo?: (() => Promise<RestoreReport>) | null;
  } = $props();

  let undoing = $state(false);

  async function handleUndo() {
    if (!onUndo) {
      onRestore();
      return;
    }
    if (undoing) return;
    undoing = true;
    try {
      const result = await onUndo();
      if (result.outcomes.length === 0) {
        // 无可确定性放回 → 退回 Finder 手动路径（不假装成功）。
        onRestore();
      } else {
        // 撤销已执行——收起 toast，结果详情由回执区呈现。
        onDismiss();
      }
    } catch {
      onRestore();
    } finally {
      undoing = false;
    }
  }
</script>

<div
  class="toast"
  role="status"
  transition:fly={{ y: 16, duration: 200 }}
>
  <span class="msg">
    已移到废纸篓 · 释放 <strong>{formatBytes(freed)}</strong>（{count} 项）
  </span>
  <button class="restore" onclick={handleUndo} disabled={undoing}>
    {undoing ? "撤销中…" : onUndo ? "撤销清理" : "在访达中恢复"}
  </button>
  <button class="dismiss" aria-label="关闭" onclick={onDismiss}>
    <svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true">
      <path d="M3 3 L9 9 M9 3 L3 9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
    </svg>
  </button>
</div>

<style>
  .toast {
    position: fixed;
    left: 50%;
    bottom: var(--sp-6);
    transform: translateX(-50%);
    z-index: 90;
    display: flex;
    align-items: center;
    gap: var(--sp-4);
    max-width: min(560px, calc(100vw - var(--sp-8)));
    padding: var(--sp-3) var(--sp-4);
    background: var(--surface-overlay);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    box-shadow: var(--elevation-2);
  }
  .msg {
    color: var(--ink-primary);
    font-size: 0.9em;
  }
  .msg strong {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .restore {
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 0.85em;
    padding: var(--sp-1) var(--sp-3);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: none;
    color: var(--accent);
    cursor: pointer;
  }
  .restore:hover {
    background: color-mix(in oklch, var(--accent) 12%, transparent);
  }
  .dismiss {
    flex: 0 0 auto;
    display: inline-flex;
    padding: var(--sp-1);
    border: none;
    background: none;
    color: var(--ink-muted);
    cursor: pointer;
  }
  .dismiss:hover {
    color: var(--ink-primary);
  }
</style>
