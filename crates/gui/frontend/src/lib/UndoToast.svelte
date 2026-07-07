<script lang="ts">
  /**
   * 撤销 toast（U5 / R11 R13 / KTD4 路 B 诚实路线）。
   *
   * 文案诚实为「已移到废纸篓」而非「已删除」；「在访达中恢复」打开系统废纸篓，让用户用
   * Finder 原生「放回原处」恢复——**不做延迟执行的假 undo**（真一键 undo 是 macOS 上的
   * 非平凡独立命题，单独立项，见 Scope Boundaries）。单实例由父组件的 toast.ts 状态保证。
   */
  import { fly } from "svelte/transition";
  import { formatBytes } from "./format";

  let {
    count,
    freed,
    onRestore,
    onDismiss,
  }: {
    count: number;
    freed: number;
    onRestore: () => void;
    onDismiss: () => void;
  } = $props();
</script>

<div
  class="toast"
  role="status"
  transition:fly={{ y: 16, duration: 200 }}
>
  <span class="msg">
    已移到废纸篓 · 释放 <strong>{formatBytes(freed)}</strong>（{count} 项）
  </span>
  <button class="restore" onclick={onRestore}>在访达中恢复</button>
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
