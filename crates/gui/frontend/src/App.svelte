<script lang="ts">
  import { checkFda, openTrash, openFdaSettings, type ProbeResult } from "./lib/ipc";
  import Clean from "./routes/Clean.svelte";
  import Purge from "./routes/Purge.svelte";
  import Uninstall from "./routes/Uninstall.svelte";
  import Analyze from "./routes/Analyze.svelte";
  import Onboarding from "./routes/Onboarding.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import type { Command } from "./lib/palette";
  import { routeCommands } from "./lib/palette-registry.svelte";

  type Boot = "checking" | "onboarding" | "ready";
  type Tab = "clean" | "purge" | "uninstall" | "analyze";

  /** statusbar 模式文案（新增 tab 只需补一行，不再叠三元链）。 */
  const TAB_LABELS: Record<Tab, string> = {
    clean: "清理模式",
    purge: "开发清理模式",
    uninstall: "卸载模式",
    analyze: "分析模式",
  };

  let boot = $state<Boot>("checking");
  let probes = $state<ProbeResult[]>([]);
  let tab = $state<Tab>("clean");

  // ---- Cmd+K 命令面板（move7 收尾）：加速器而非唯一入口，四 tab 可见导航保留（R7）----
  let paletteOpen = $state(false);
  // 打开前触发面板的元素；关闭时焦点还原到它，不留焦点陷阱残留（R4）。
  let paletteTrigger: HTMLElement | null = null;

  /** 静态命令：4 导航 + 2 全局动作（KTD1/KTD3），不依赖路由内部状态。 */
  const staticCommands: Command[] = [
    { id: "nav.clean", title: "清理", keywords: ["clean", "qingli"], run: () => (tab = "clean") },
    { id: "nav.purge", title: "开发清理", keywords: ["purge", "dev", "kaifa"], run: () => (tab = "purge") },
    { id: "nav.uninstall", title: "卸载", keywords: ["uninstall", "xiezai"], run: () => (tab = "uninstall") },
    { id: "nav.analyze", title: "分析", keywords: ["analyze", "fenxi"], run: () => (tab = "analyze") },
    { id: "act.trash", title: "打开废纸篓", keywords: ["trash", "feizhilou"], run: () => void openTrash() },
    { id: "act.fda", title: "打开磁盘访问权限设置", keywords: ["fda", "permission", "quanxian"], run: () => void openFdaSettings() },
  ];

  // 面板命令 = 静态 + 当前路由动态注册（KTD1）。路由命令随其相位/选择态反应式增删（KTD2）。
  const commands = $derived<Command[]>([...staticCommands, ...routeCommands()]);

  function openPalette() {
    paletteTrigger = document.activeElement as HTMLElement | null;
    paletteOpen = true;
  }
  function closePalette() {
    paletteOpen = false;
    // R4 焦点还原：若触发元素已随导航命令卸载（focus() 对脱离 DOM 的节点静默失败，
    // 焦点会落到 body），回退聚焦当前激活 tab——审查发现，correctness+julik+adversarial 三家一致。
    const target = paletteTrigger?.isConnected
      ? paletteTrigger
      : document.querySelector<HTMLElement>(".tab.active");
    target?.focus();
    paletteTrigger = null;
  }

  function onGlobalKey(e: KeyboardEvent) {
    // Cmd+K（macOS 主路径）/ Ctrl+K；仅主界面（ready）唤起，onboarding/checking 不响应（R1）。
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      if (boot !== "ready") return;
      if (e.repeat) return; // 长按 K 自动重复不应反复 toggle（审查：adversarial）
      e.preventDefault(); // 拦截 webview 默认（部分 Ctrl+K 聚焦地址栏）
      if (paletteOpen) closePalette();
      else openPalette();
    }
  }

  $effect(() => {
    window.addEventListener("keydown", onGlobalKey);
    return () => window.removeEventListener("keydown", onGlobalKey);
  });

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
        <button class="tab" class:active={tab === "purge"} onclick={() => (tab = "purge")}>
          开发清理
        </button>
        <button class="tab" class:active={tab === "uninstall"} onclick={() => (tab = "uninstall")}>
          卸载
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
    {:else if tab === "purge"}
      <Purge />
    {:else if tab === "uninstall"}
      <Uninstall />
    {:else}
      <Analyze />
    {/if}
  </main>

  <footer class="statusbar">
    <span class="hint">macCleaner · 移废纸篓可恢复 · 零遥测</span>
    <span class="mode">{boot === "ready" ? TAB_LABELS[tab] : ""}</span>
  </footer>

  {#if paletteOpen}
    <CommandPalette {commands} onClose={closePalette} />
  {/if}
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
