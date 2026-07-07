/**
 * 相位过渡包裹（KTD1）。有 View Transitions API 且未开启 reduced-motion 时，
 * 用 `document.startViewTransition` 做一次跨相位淡切；否则瞬切（同步执行 update）。
 *
 * 注意：实际 Tauri 目标是 WKWebView（Safari 15.4+），View Transitions 直到 Safari 18
 * 才可用——此处刻意做能力探测优雅降级，缺失即瞬切，不引入 spinner/编排（R19）。
 */
type ViewTransitionDoc = Document & {
  startViewTransition?: (cb: () => void) => unknown;
};

export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

export function withViewTransition(update: () => void): void {
  const doc = document as ViewTransitionDoc;
  if (prefersReducedMotion() || typeof doc.startViewTransition !== "function") {
    update();
    return;
  }
  doc.startViewTransition(update);
}
