import { defineConfig, devices } from "@playwright/test";

// 前端 E2E（U1 / R1）：真实 Chromium 渲染真实 Svelte 界面，Tauri IPC 边界由 e2e/support 的
// mock 拦截（KTD2）。不走原生 Tauri 窗口——macOS 无 WKWebView driver（研究结论），也非本轮目标。
export default defineConfig({
  testDir: "./e2e",
  // 只认 *.spec.ts 为 Playwright 用例；contract.test.ts 归 vitest（见 vite.config test.include）。
  testMatch: /.*\.spec\.ts$/,
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
  },
  // 默认用 Playwright 自带 chromium（标准范式，冷环境先 `npx playwright install chromium`）。
  // 逃生口：设 PW_CHANNEL=chrome 改用系统 Google Chrome，免下载——仅供本机 CDN 不可达时用。
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        channel: process.env.PW_CHANNEL || undefined,
      },
    },
  ],
  // vite.config strictPort:true（1420）：已有 dev server 时 reuseExistingServer 复用之，
  // 避免冷/热环境端口冲突（feasibility review）。PW_NO_WEBSERVER 供已手动起 vite 时跳过托管。
  webServer: process.env.PW_NO_WEBSERVER
    ? undefined
    : {
        command: "pnpm dev",
        url: "http://localhost:1420",
        reuseExistingServer: !process.env.CI,
        timeout: 120_000,
      },
});
