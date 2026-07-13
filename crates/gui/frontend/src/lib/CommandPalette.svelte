<script lang="ts">
  /**
   * 命令面板浮层（Cmd+K，U2 / move7 收尾）。
   *
   * 开发者加速器：键盘唤起、模糊匹配、键盘导航跨功能跳转。ideation #7 护栏——
   * 面板是加速器不是唯一入口（四 tab 可见导航由 App 保留），且**半成品比没有更糟**：
   * 模糊匹配（palette.ts）、焦点陷阱、与全局 modal（ConfirmDelete）视觉一致，三者齐备。
   *
   * 焦点模型：焦点恒定停在搜索输入（Raycast 式）——↑/↓ 移动列表高亮而非移动焦点，
   * Tab/Shift+Tab 被拦截（preventDefault）使焦点永不逃出面板（R5 焦点陷阱）。
   * 关闭与关闭后的焦点还原由父层 App 负责（R4）。
   */
  import { fuzzyFilter, type Command } from "./palette";

  let {
    commands,
    onClose,
  }: {
    commands: Command[];
    onClose: () => void;
  } = $props();

  let query = $state("");
  let selectedIndex = $state(0);
  let listEl = $state<HTMLUListElement | null>(null);

  const filtered = $derived(fuzzyFilter(commands, query));

  // query 变化后结果集重排：高亮回到首项，并夹逼进合法区间（防越界）。
  $effect(() => {
    void query;
    selectedIndex = 0;
  });

  function run(cmd: Command) {
    cmd.run();
    onClose();
  }

  function move(delta: number) {
    const n = filtered.length;
    if (n === 0) return;
    selectedIndex = (selectedIndex + delta + n) % n; // 首尾环绕
    // 高亮项滚入可视区。
    queueMicrotask(() => {
      listEl?.querySelector<HTMLElement>('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
    });
  }

  function handleKeys(e: KeyboardEvent) {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "Enter":
        e.preventDefault();
        if (filtered[selectedIndex]) run(filtered[selectedIndex]);
        break;
      case "Escape":
        e.preventDefault();
        onClose();
        break;
      case "Tab":
        // 焦点陷阱：拦截 Tab，焦点恒留在输入框，绝不逃出面板（R5）。
        e.preventDefault();
        break;
    }
  }
</script>

<div
  class="backdrop"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <!-- 键盘在容器层统一处理；焦点由内部 input 持有 -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="palette" role="dialog" aria-modal="true" aria-label="命令面板" tabindex="-1" onkeydown={handleKeys}>
    <!-- svelte-ignore a11y_autofocus -->
    <input
      type="text"
      bind:value={query}
      placeholder="搜索命令…"
      spellcheck="false"
      autocapitalize="off"
      autocomplete="off"
      aria-label="搜索命令"
      autofocus
    />

    {#if filtered.length}
      <ul class="results" role="listbox" aria-label="命令" bind:this={listEl}>
        {#each filtered as cmd, i (cmd.id)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            role="option"
            aria-selected={i === selectedIndex}
            class:active={i === selectedIndex}
            onclick={() => run(cmd)}
            onmousemove={() => (selectedIndex = i)}
          >
            {cmd.title}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="empty">无匹配命令</p>
    {/if}
  </div>
</div>

<style>
  /* 视觉与 ConfirmDelete 全局 modal 一致：同 backdrop、surface-overlay、radius、阴影。
     边框用中性 --border-subtle（非 --state-danger）——红只跟随 Risky/不可逆语义（R6 / R18）。 */
  .backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in oklch, var(--surface-base) 70%, transparent);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    z-index: 100;
    padding: 12vh var(--sp-6) var(--sp-6);
  }
  .palette {
    background: var(--surface-overlay);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    width: min(560px, 100%);
    max-height: min(60vh, 100%);
    display: flex;
    flex-direction: column;
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.5);
    overflow: hidden;
  }
  input {
    width: 100%;
    box-sizing: border-box;
    padding: var(--sp-3) var(--sp-4);
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--ink-primary);
    font-family: var(--font-ui);
    font-size: 1em;
  }
  input:focus {
    outline: none;
    border-bottom-color: var(--accent);
  }
  .results {
    list-style: none;
    margin: 0;
    padding: var(--sp-1);
    overflow-y: auto;
    flex: 1 1 auto;
  }
  .results li {
    display: flex;
    align-items: center;
    min-height: var(--row-height);
    padding: var(--sp-1) var(--sp-3);
    border-radius: 4px;
    color: var(--ink-primary);
    cursor: pointer;
    font-family: var(--font-ui);
  }
  .results li.active {
    background: color-mix(in oklch, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .empty {
    margin: 0;
    padding: var(--sp-4);
    color: var(--ink-muted);
    font-size: 0.9em;
  }
</style>
