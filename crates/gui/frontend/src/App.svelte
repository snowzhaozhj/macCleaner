<script lang="ts">
  import { checkFda, type ProbeResult } from "./lib/ipc";
  import Clean from "./routes/Clean.svelte";
  import Analyze from "./routes/Analyze.svelte";
  import Onboarding from "./routes/Onboarding.svelte";

  type Boot = "checking" | "onboarding" | "ready";
  type Tab = "clean" | "analyze";

  let boot = $state<Boot>("checking");
  let probes = $state<ProbeResult[]>([]);
  let tab = $state<Tab>("clean");

  async function runFdaCheck() {
    boot = "checking";
    try {
      const status = await checkFda();
      probes = status.probes;
      boot = status.authorized ? "ready" : "onboarding";
    } catch (e) {
      // check_fda 不可用时不阻断使用（MVP）：直接进主界面，仅在控制台留痕。
      console.warn("check_fda 失败，跳过引导：", e);
      boot = "ready";
    }
  }

  // 启动即检查 FDA
  void runFdaCheck();
</script>

<div class="shell">
  <header class="titlebar">
    <div class="brand">
      <span class="logo">mc</span>
      <span class="title">macCleaner</span>
    </div>
    {#if boot === "ready"}
      <nav class="tabs" aria-label="功能切换">
        <button class="tab" class:active={tab === "clean"} onclick={() => (tab = "clean")}>
          清理
        </button>
        <button
          class="tab explore"
          class:active={tab === "analyze"}
          onclick={() => (tab = "analyze")}
        >
          分析
        </button>
      </nav>
    {/if}
  </header>

  <main class="content">
    {#if boot === "checking"}
      <div class="checking">
        <span>检查磁盘访问权限…</span>
      </div>
    {:else if boot === "onboarding"}
      <Onboarding {probes} onRecheck={() => (boot = "ready")} />
    {:else if tab === "clean"}
      <Clean />
    {:else}
      <Analyze />
    {/if}
  </main>

  <footer class="statusbar">
    <span class="hint">macCleaner · 移废纸篓可恢复 · 零遥测</span>
    <span class="mode">{boot === "ready" ? (tab === "clean" ? "清理模式" : "分析模式") : ""}</span>
  </footer>
</div>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--accent);
    background: var(--surface-raised);
    flex: 0 0 auto;
  }
  .brand {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }
  .logo {
    font-family: var(--font-mono);
    font-weight: 700;
    color: var(--accent);
    background: color-mix(in oklch, var(--accent) 16%, transparent);
    padding: 2px var(--sp-2);
    border-radius: 4px;
  }
  .title {
    font-weight: 600;
  }
  .tabs {
    display: flex;
    gap: var(--sp-1);
  }
  .tab {
    font-family: var(--font-ui);
    font-size: 0.9em;
    padding: var(--sp-1) var(--sp-4);
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--ink-muted);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--ink-primary);
  }
  .tab.active {
    color: var(--accent);
    border-color: var(--accent);
    background: color-mix(in oklch, var(--accent) 10%, transparent);
  }
  .tab.explore.active {
    color: var(--accent-explore);
    border-color: var(--accent-explore);
    background: color-mix(in oklch, var(--accent-explore) 10%, transparent);
  }
  .content {
    flex: 1 1 auto;
    min-height: 0;
    padding: var(--sp-4);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .content > :global(*) {
    flex: 1 1 auto;
    min-height: 0;
  }
  .checking {
    margin: auto;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    color: var(--ink-muted);
  }
  .statusbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--sp-1) var(--sp-4);
    border-top: 1px solid var(--border-subtle);
    color: var(--ink-muted);
    font-size: 0.8em;
    flex: 0 0 auto;
  }
  .mode {
    font-family: var(--font-mono);
  }
</style>
