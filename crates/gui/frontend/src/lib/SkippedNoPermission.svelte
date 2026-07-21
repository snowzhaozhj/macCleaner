<script lang="ts">
  // 「因权限跳过 N 项」展示块——Analyze / Orphans / Uninstall 三入口共用（#23 权限跳过展示对齐）。
  // 与 Clean/Purge 的内联块同构（同 toggle + 列表 + 样式）；clean/purge 保留内联（计划边界），
  // 故此组件是三入口的去重载体，五入口行为一致。跳过项是**只读展示**，不可勾选、永不进待删集
  // （计划 R5：删除授权只读 categories[].items，从不读此列表）。
  // FDA 引导按钮（skip-fda-guide 计划 R1/R3）：只跳转系统设置，绝不触碰 selected/marked。
  import { openFdaSettings } from "./ipc";

  let { skipped }: { skipped: string[] } = $props();
  let show = $state(false);
</script>

{#if skipped.length > 0}
  <div class="skipped">
    <button class="link" onclick={() => (show = !show)}>
      因权限跳过 {skipped.length} 项 {show ? "收起" : "展开"}
    </button>
    {#if show}
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

<style>
  .skipped {
    padding: var(--sp-3) 0 0;
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
