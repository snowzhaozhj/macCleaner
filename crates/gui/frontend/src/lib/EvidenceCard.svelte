<script lang="ts">
  /**
   * 常驻证据文案（U6 / R14 / R17 / R18）。
   *
   * 每项默认可见一行 `impact · recovery`（如「缓存 · 会自动重建」），无需 hover/展开。
   * 刻意用**无衬线 muted 文字**而非填色 chip（R18：安全色不做装饰，红色只跟随 Risky 语义）；
   * 等宽字体只留给路径/体积（R17 字体管辖权），证据是叙述性标签故用系统无衬线。
   * 空文案优雅降级——两者皆空则不渲染任何节点（edge，不留空行）。
   */
  let {
    impact = "",
    recovery = "",
  }: { impact?: string; recovery?: string } = $props();

  const parts = $derived(
    [impact, recovery].map((s) => s?.trim()).filter((s): s is string => !!s),
  );
</script>

{#if parts.length > 0}
  <span class="evidence">{parts.join(" · ")}</span>
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
</style>
