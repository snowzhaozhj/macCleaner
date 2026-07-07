<script lang="ts">
  /**
   * 路径文本（U3 / R6 / R17）。等宽显示，父目录从末尾省略、basename 始终完整可见。
   * 取代原 `direction:rtl` 截断 hack——用正常 ltr 布局 + flex 收缩达成「保留尾部」，
   * 无 bidi 副作用（rtl hack 会让含数字/英文的路径显示错乱）。
   */
  import { splitPath } from "./format";

  let { path }: { path: string } = $props();
  const parts = $derived(splitPath(path));
</script>

<span class="path" title={path}>
  <span class="head">{parts.head}</span><span class="tail">{parts.tail}</span>
</span>

<style>
  .path {
    display: inline-flex;
    min-width: 0;
    flex: 1 1 auto;
    font-family: var(--font-mono);
    font-size: 0.85em;
    white-space: nowrap;
  }
  .head {
    flex: 0 1 auto;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--ink-muted);
  }
  .tail {
    flex: 0 0 auto;
    color: var(--ink-primary);
  }
</style>
