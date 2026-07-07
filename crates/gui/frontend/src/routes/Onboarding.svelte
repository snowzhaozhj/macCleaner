<script lang="ts">
  import { openFdaSettings, checkFda, type ProbeResult } from "../lib/ipc";

  // authorized 后需重启应用才生效（macOS TCC 缓存）——所以这里只做"重新检查"提示，
  // 由 App 决定何时进入主界面。
  let { probes, onRecheck }: { probes: ProbeResult[]; onRecheck: () => void } =
    $props();

  let rechecking = $state(false);

  async function recheck() {
    rechecking = true;
    try {
      const status = await checkFda();
      if (status.authorized) {
        onRecheck();
      } else {
        probes = status.probes;
      }
    } finally {
      rechecking = false;
    }
  }

  function statusText(p: ProbeResult): string {
    switch (p.status.status) {
      case "readable":
        return "可读";
      case "no_permission":
        return "无权限";
      case "missing":
        return "不存在";
      case "error":
        return `错误：${p.status.detail}`;
    }
  }
</script>

<div class="onboarding">
  <div class="card">
    <h1>需要完全磁盘访问权限</h1>
    <p class="lede">
      macCleaner 需要「完全磁盘访问」（Full Disk Access）才能扫描受保护的系统与浏览器缓存。
      未授权时只能看到部分目录，清理结果会不完整。
    </p>

    <ol class="steps">
      <li>点击下方按钮打开「系统设置 › 隐私与安全性 › 完全磁盘访问」。</li>
      <li>在列表中启用 <strong>macCleaner</strong>。</li>
      <li>授权后<strong>重启本应用</strong>使权限生效（macOS 权限缓存所致）。</li>
    </ol>

    {#if probes.length > 0}
      <div class="probes">
        <p class="probes-title">权限探测：</p>
        <ul>
          {#each probes as p (p.path)}
            <li class:ok={p.status.status === "readable"}>
              <span class="probe-status">{statusText(p)}</span>
              <span class="probe-path" title={p.path}>{p.path}</span>
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    <div class="actions">
      <button class="primary" onclick={openFdaSettings}>打开系统设置</button>
      <button onclick={recheck} disabled={rechecking}>
        {rechecking ? "检查中…" : "已授权，重新检查"}
      </button>
    </div>
    <p class="hint">若重新检查后仍未生效，请完全退出并重启 macCleaner。</p>
  </div>
</div>

<style>
  .onboarding {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: var(--sp-6);
  }
  .card {
    max-width: 560px;
    background: var(--surface-raised);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    padding: var(--sp-8);
  }
  h1 {
    margin: 0 0 var(--sp-4);
    font-size: 1.3rem;
    color: var(--state-warning);
  }
  .lede {
    color: var(--ink-primary);
    margin: 0 0 var(--sp-4);
  }
  .steps {
    color: var(--ink-muted);
    padding-left: var(--sp-6);
    margin: 0 0 var(--sp-6);
    line-height: 1.8;
  }
  .steps strong {
    color: var(--ink-primary);
  }
  .probes {
    background: var(--surface-base);
    border-radius: var(--radius);
    padding: var(--sp-3);
    margin-bottom: var(--sp-6);
  }
  .probes-title {
    margin: 0 0 var(--sp-2);
    color: var(--ink-muted);
    font-size: 0.85em;
  }
  .probes ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .probes li {
    display: flex;
    gap: var(--sp-3);
    align-items: baseline;
    font-family: var(--font-mono);
    font-size: 0.8em;
    padding: 2px 0;
  }
  .probe-status {
    flex: 0 0 5em;
    color: var(--state-danger);
  }
  .probes li.ok .probe-status {
    color: var(--safety-safe);
  }
  .probe-path {
    color: var(--ink-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
    margin-bottom: var(--sp-3);
  }
  button {
    font-family: var(--font-ui);
    font-size: 0.9em;
    padding: var(--sp-2) var(--sp-4);
    border-radius: var(--radius);
    border: 1px solid var(--border-subtle);
    background: var(--surface-overlay);
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
  .hint {
    margin: 0;
    color: var(--ink-faint);
    font-size: 0.8em;
  }
</style>
