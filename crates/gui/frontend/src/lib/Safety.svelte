<script lang="ts">
  import type { SafetyLevel } from "./ipc";
  import { safetyDescriptor } from "./safety";

  // 三通道恒同现：字形 + 文字标签 + OKLCH 色。永不退化为纯色块。
  let { safety, compact = false }: { safety: SafetyLevel; compact?: boolean } =
    $props();

  const d = $derived(safetyDescriptor(safety));
</script>

<span
  class="safety"
  class:compact
  style="color: {d.tokenVar}"
  title="安全等级：{d.label}"
>
  <span class="glyph" aria-hidden="true">{d.glyph}</span>
  <span class="label">{d.label}</span>
</span>

<style>
  .safety {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    font-family: var(--font-mono);
    font-size: 0.85em;
    white-space: nowrap;
  }
  .glyph {
    font-size: 0.9em;
    line-height: 1;
  }
  /* compact 仍保留文字标签（三通道不变量），仅用于窄列微调间距 */
  .compact .label {
    font-size: 0.9em;
  }
</style>
