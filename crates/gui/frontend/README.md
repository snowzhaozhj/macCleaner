# maccleaner-frontend

macCleaner GUI 前端（Tauri 2 + Svelte 5 + Vite）。

## 自测入口（交付前必跑）

一条命令跑完冒烟 + 单测 + 契约 + E2E，任一失败非零退出：

```bash
pnpm selftest
```

它串联：`svelte-check`（类型）→ `vite build`（构建）→ `vitest run`（纯逻辑 + IPC 契约守卫）→ `playwright install chromium`（幂等）→ `playwright test`（前端行为 E2E）。

**Claude / 任何改 GUI 的人：声明「GUI 工作完成」前必须跑通 `selftest`，红则不得交付。**

### E2E 说明

- **真浏览器、mock 后端**：Playwright 在真实浏览器里渲染真实 Svelte 界面、执行真实点击，Tauri IPC 边界（`invoke` / `Channel`）由 `e2e/support/tauri-mock.ts` 拦截、`e2e/support/fixtures.ts` 回放事件流。**不走真实 Tauri 窗口 / 真实后端 / 真实删除**（macOS 无 WKWebView driver；也非本轮目标）。
- **浏览器**：标准范式，用 Playwright 自带 chromium。`selftest` 会先跑 `playwright install chromium`（幂等，已装则秒过）。首次下载走 `cdn.playwright.dev` 的 cft 路径（约 170MB，经公司云壳流量检测会偏慢，耐心等）。逃生口：`PW_CHANNEL=chrome pnpm e2e` 改用系统 Google Chrome，免下载。
- **契约守卫**（`e2e/contract.test.ts`，vitest）：静态比对 `ipc.ts` 调用 ⇔ Rust `generate_handler!` + 命令签名，命令名/参数漂移即红。它只钉签名层；事件/返回值的 serde 形状漂移不在其内（见 OQ4）。

### 已知环境限制（受限 shell）

某些受限 shell（如 agent 沙箱）里 Playwright 托管的 `webServer`（自动起 `pnpm dev`）会静默卡死。此时手动分两步：

```bash
pnpm dev &                          # 另起 vite
PW_NO_WEBSERVER=1 pnpm e2e          # 复用已起的 vite，跳过托管
```

普通开发机 / CI 直接 `pnpm selftest` 即可，无需此变通。
