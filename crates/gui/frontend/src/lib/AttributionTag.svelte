<script lang="ts">
  /**
   * Analyze 节点行的只读归因标注：命中内置清理规则时展示「分类名 + 安全三通道」，
   * 未命中展示中性「未识别」。**纯认知辅助**——不改标记、预选或删除授权（删除仍走
   * classifyMarked → deleteMarked 的 fail-closed 重分类）。「未识别」是诚实的中性态，
   * 绝不渲染成「可安全删除」暗示。
   */
  import type { NodeAttribution } from "./ipc";
  import { safetyDescriptor } from "./safety";

  let { attribution }: { attribution: NodeAttribution | undefined } = $props();

  // 命中 = category 与 safety 同时存在（后端一一对应，None 时两者皆 null）。
  const hit = $derived(
    attribution?.category != null && attribution.safety != null,
  );
  const descriptor = $derived(
    hit && attribution?.safety ? safetyDescriptor(attribution.safety) : null,
  );
</script>

{#if hit && descriptor && attribution}
  <span
    class="attribution hit"
    style="color: {descriptor.tokenVar}"
    title="归属清理分类：{attribution.category} · 安全等级：{descriptor.label}"
  >
    <span class="glyph" aria-hidden="true">{descriptor.glyph}</span>
    <span class="category">{attribution.category}</span>
  </span>
{:else}
  <span class="attribution unknown" title="未匹配任何内置清理规则">未识别</span>
{/if}

<style>
  .attribution {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 0.72em;
    white-space: nowrap;
  }
  .glyph {
    font-family: var(--font-mono);
    font-size: 0.9em;
    line-height: 1;
  }
  .unknown {
    color: var(--ink-faint);
  }
</style>
