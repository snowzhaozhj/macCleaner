import { defineConfig } from "vite";
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
});
