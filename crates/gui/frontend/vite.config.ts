// 用 vitest/config 的 defineConfig（兼容 vite，额外支持 `test` 键）以便限定 vitest 只跑
// 单测（*.test.ts），把 Playwright 的 *.spec.ts 排除在外——否则 vitest 默认 glob 会误抓
// e2e/*.spec.ts（Playwright 用例，非 vitest）导致整套单测崩。dev/build 行为不变。
import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri 期望前端固定端口 1420（devUrl）；清屏关掉以免遮住 Rust 侧日志。
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // 目标对齐 WKWebView（Safari 15.4+），OKLCH 原生可用。
    target: "safari15",
  },
  test: {
    // 只跑 *.test.ts（含 e2e/contract.test.ts 契约守卫）；Playwright 的 *.spec.ts 归 playwright。
    include: ["src/**/*.test.ts", "e2e/**/*.test.ts"],
  },
});
