<script lang="ts">
  // 「因权限跳过 N 项」展示块——五入口共用(#23 权限跳过展示对齐)。
  // 与 Clean/Purge 内联块同构:折叠按钮 + 可展开路径列表。跳过项是**只读展示**,
  // 不可勾选、永不进待删集(计划 R5:删除授权只读 categories[].items,从不读此列表)。
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
</style>
