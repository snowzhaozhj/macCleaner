<script lang="ts">
  /**
   * 清理回执（U7 / R15 R16）。数据源 = `CleanReport`（后端权威），排版化呈现：
   * 释放量 + 成功计数 + 去向废纸篓 + 如何恢复；失败项优雅分列（继承逐项优雅降级语义）。
   * 刻意**无 confetti、无大绿勾 hero**（不庆祝、不表演）；开源/零遥测归 About 不放这里（R16）。
   */
  import { formatBytes, summarizeReport } from "./format";
  import type { CleanReport } from "./ipc";
  import PathText from "./PathText.svelte";

  let {
    report,
    onRestore,
  }: {
    report: CleanReport;
    onRestore: () => void;
  } = $props();

  const r = $derived(summarizeReport(report));
</script>

<div class="receipt">
  <p class="line">
    已释放 <strong class="freed">{formatBytes(r.freed)}</strong>
    <span class="muted">· {r.successCount} 项已移到废纸篓</span>
  </p>
  <p class="restore-hint">
    文件在废纸篓中可恢复。<button class="link" onclick={onRestore}>在访达中恢复</button>
  </p>

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
  /* 释放量用中性成功色的等宽数字，非填色 hero；不喧宾（R18） */
  .freed {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 700;
    color: var(--state-success);
  }
  .muted {
    color: var(--ink-muted);
    font-size: 0.9rem;
  }
  .restore-hint {
    margin: 0;
    color: var(--ink-muted);
    font-size: 0.9em;
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
  .link:hover {
    text-decoration: underline;
  }
  .failures {
    padding-top: var(--sp-3);
    border-top: 1px solid var(--border-subtle);
  }
  .fail-head {
    margin: 0 0 var(--sp-2);
    color: var(--ink-muted);
    font-size: 0.9em;
  }
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
  .failures li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    min-width: 0;
  }
  .fail-reason {
    flex: 0 0 auto;
    font-size: 0.8em;
    color: var(--ink-faint);
  }
</style>
