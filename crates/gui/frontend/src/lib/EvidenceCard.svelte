<script lang="ts">
  /**
   * 常驻证据文案（U6 / R14 / R17 / R18）。
   *
   * 每项默认可见一行 `impact · recovery`（如「缓存 · 会自动重建」），无需 hover/展开。
   * 刻意用**无衬线 muted 文字**而非填色 chip（R18：安全色不做装饰，红色只跟随 Risky 语义）；
   * 等宽字体只留给路径/体积（R17 字体管辖权），证据是叙述性标签故用系统无衬线。
   * 空文案优雅降级——两者皆空则不渲染任何节点（edge，不留空行）。
   *
   * `full`（move 6 审查面孔）：折叠列表行用单行截断（决策语境）；展开审查态传 `full`，
   * 去截断、按 impact/recovery 分两行完整呈现（回答「它到底是什么」）。
   */
  let {
    impact = "",
    recovery = "",
    full = false,
  }: { impact?: string; recovery?: string; full?: boolean } = $props();

  const parts = $derived(
    [impact, recovery].map((s) => s?.trim()).filter((s): s is string => !!s),
  );
</script>

{#if parts.length > 0}
  {#if full}
    <span class="evidence full">
      {#each parts as part (part)}
        <span class="line">{part}</span>
      {/each}
    </span>
  {:else}
    <span class="evidence">{parts.join(" · ")}</span>
  {/if}
{/if}

<style>
  .evidence {
    font-family: var(--font-ui);
    font-size: 0.8em;
    color: var(--ink-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* 审查面孔：去截断、完整换行，impact/recovery 分行呈现 */
  .evidence.full {
    display: flex;
    flex-direction: column;
    gap: 2px;
    white-space: normal;
    overflow: visible;
    line-height: 1.5;
  }
  .evidence.full .line {
    white-space: normal;
  }
</style>
