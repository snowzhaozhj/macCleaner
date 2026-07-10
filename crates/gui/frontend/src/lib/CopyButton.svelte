<script lang="ts">
  /**
   * 复制到剪贴板的小按钮（move 6 审查面孔 / R3 R4）。路径与等价命令共用一个原语。
   *
   * 用 `navigator.clipboard.writeText`（由点击这一用户手势触发，WKWebView 允许），
   * 不引入 tauri clipboard 插件与新 capability。复制后 1.5s 内显示「已复制」反馈；
   * 失败降级为标题提示而非静默崩溃（复制非关键路径）。
   */
  let {
    text,
    label = "复制",
  }: { text: string; label?: string } = $props();

  let copied = $state(false);
  let failed = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      copied = true;
      failed = false;
    } catch {
      failed = true;
      copied = false;
    }
    clearTimeout(timer);
    timer = setTimeout(() => {
      copied = false;
      failed = false;
    }, 1500);
  }
</script>

<button
  class="copy"
  class:copied
  onclick={copy}
  title={failed ? "复制失败" : copied ? "已复制" : label}
  aria-label={label}
>
  {#if copied}已复制{:else if failed}失败{:else}{label}{/if}
</button>

<style>
  .copy {
    flex: 0 0 auto;
    font-family: var(--font-ui);
    font-size: 0.75em;
    padding: 2px var(--sp-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius);
    background: var(--surface-base);
    color: var(--ink-muted);
    cursor: pointer;
    transition: color var(--dur-fast) var(--ease-out-quart);
  }
  .copy:hover {
    color: var(--ink-primary);
    border-color: var(--ink-muted);
  }
  .copy.copied {
    color: var(--accent);
    border-color: var(--accent);
  }
</style>
