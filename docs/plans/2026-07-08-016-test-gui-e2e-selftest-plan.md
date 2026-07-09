---
title: GUI 前端 E2E 自测骨架（无头浏览器·Tauri 边界 mock） - Plan
type: test
date: 2026-07-08
topic: gui-e2e-selftest
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-brainstorm
execution: code
---

# GUI 前端 E2E 自测骨架（无头浏览器·Tauri 边界 mock） - Plan

## Goal Capsule

- **Objective:** 为 `crates/gui`（Tauri 2 + Svelte 5）建一套**无头真浏览器 E2E 自测骨架**，让「GUI 已自测、可交付」这句话可信——由 Claude 在每次交付 GUI 前自己先跑，把用户从手动点按 QA 的回路里移出去。本轮只保**功能正确性**（按钮真触发正确命令、流程跑通、UI 正确响应）+ IPC 契约守卫，卡顿性能留待办。
- **Product authority:** 用户（产品负责人）。
- **Open blockers:** 无。

> **Product Contract preservation:** unchanged — R1–R5 / SC1–SC5 文本与 ID 保留。规划期定夺了两个 Outstanding：OQ2（覆盖广度 = 主干路径 + 关键分支）、OQ3（契约守卫 = 静态解析）。OQ1（卡顿阈值）仍 deferred。OQ4 系本轮规划新增（源自 Risks & Mitigations 的语义漂移风险），非 brainstorm 遗留。

> **背景（诚实记录）:** 当前分支 `feat/gui-redesign-v1` 的 GUI 改动**从未被实际运行验证过**。现有 6 个 vitest 只测纯逻辑（aggregate/format/toast/safety/categories/confirm），对「按钮点不了 / 卡得要死」这类集成/运行时故障零覆盖。本计划即为堵这个洞。**假设 A1（未核实）**：当前分支确有真实运行时 bug；骨架搭建将确认并定位，若某症状复现不出则如实记「无法复现」而非硬造测试。

---

## Product Contract

### 要解决的问题

GUI 的「按钮→后端」整条路（`ipc.ts` 的 `invoke(...)` → Rust `#[tauri::command]`）当前无任何测试。`ipc.ts` 的 TS 类型是**手工逐字镜像** mc-core 的 serde 形状——命令名、参数名、事件形状任一漂移，按钮就静默失效，而纯逻辑单测全绿、毫无察觉。用户为此被迫充当手动 QA。

### 主要用户 / 使用者

- **直接使用者：Claude（本 agent）**——在声明「GUI 工作完成」前自动跑此骨架，失败即不得交付。
- **受益人：产品负责人（用户）**——不再需要手动逐个点按验证 GUI。

### 期望结果

一条命令跑完整套无头自测，任一失败即非零退出。Claude 据此在交付前自证「按钮真能用、关键流程跑得通」。

### 本轮范围（做什么）

- **R1｜真浏览器 E2E 骨架**：用真实浏览器（Playwright）渲染真实 Svelte 界面，执行真实点击/输入/滚动。
- **R2｜Tauri 边界夹具后端**：在 IPC 边界（`invoke` / `Channel`）挂 mock，回放真实规模的 `ProgressEvent` / `AnalyzeEvent` 事件流。不走真实 Rust 后端、不做真实删除。
- **R3｜关键流功能断言**——每个按钮/动作断言「触发正确命令 + 正确参数（名+形状）+ 正确 UI 响应」，覆盖四条流（**主干路径 + 关键分支**，非穷举每个组件）：
  - ① Onboarding / FDA 权限门（`check_fda` → `open_fda_settings`）。
  - ② Clean：`scan_clean` 流 → `StreamingList` 填充 → 勾选 → `ConfirmDelete` →（含 Risky 必走 type-to-confirm，输入 `delete` 口令）→ `clean` → `CleanReceipt` → `UndoToast`（`open_trash`）。
  - ③ Analyze：`analyze` 流 → 树渲染 → 标记 → `classify_marked` → `delete_marked`。
  - ④ 扫描中途取消（`cancel_scan`）。
- **R4｜IPC 契约守卫（信任锚）**：校验前端调用的命令名 + 参数名集合，与 Rust `#[tauri::command]` 注册表（`generate_handler!`）及签名一致。Rust 改名/改参而前端未跟，守卫必须红。
- **R5｜单命令入口**：整套经一条命令运行、无头、失败非零退出，可被 Claude 在交付前无人值守调用。

### 明确不做（Out of scope）

- **真实 Tauri 窗口 / 原生 WKWebView E2E**——用户明确砍掉；且研究证实 macOS 无 WKWebView driver（`tauri-driver` 仅 Win/Linux 支持），此路本就不通。
- **真实后端 / 真实删除到废纸篓**——全程 mock IPC 边界。
- **卡顿 / 性能预算断言**——本轮不做（OQ1，留待办）。
- **TUI**、**视觉回归截图 diff**、**每组件空状态/边角穷举**。

### 关键决策与假设（产品层）

- **KD1｜E2E = 真浏览器（Playwright）**，非进程内组件测试（用户在 A/B 间选 A）。
- **KD2｜Forcing function：骨架第一个交付证明必须在当前分支上跑红**——能复现「按钮点不了」等现存故障。先让它红、定位真 bug，修完再让它绿。

### 成功判据

- **SC1**：一条命令跑完整套、无头、失败非零退出。
- **SC2**：骨架建成时在当前（未修）分支上失败，证明抓得住真 bug；修复后转绿。**判据限定**：此「跑红」只针对骨架可探测的类别——前端接线（按钮未调 invoke / 参数错 / UI 未响应）与命令契约；WKWebView-only 渲染、真实后端-only、卡顿(OQ1) 明确不在骨架覆盖内，此类症状「复现不出」是合法结果，不视为 SC2 失败。
- **SC3**：四条关键流的每个按钮/动作都有断言（正确命令 + 正确参数 + 正确 UI 响应）。
- **SC4**：IPC 契约守卫在命令名/参数与 Rust 分歧时红。
- **SC5**：Risky type-to-confirm 门被覆盖——未输入 `delete` 口令时不得删除 Risky 项。

---

## Planning Contract

### 技术现状（研究结论）

- **栈**：Tauri 2 + Svelte 5（runes：`$state`）+ Vite 6 + TS strict。前端在 `crates/gui/frontend`，dev 固定端口 **1420**（`vite.config.ts` strictPort）。
- **已有测试**：`vitest run`（`package.json` 的 `test`）跑 6 个纯逻辑 `.test.ts`（`src/lib/*`）。**无组件渲染测试、无 E2E、无契约校验**。测试风格：`describe/it/expect`，中文用例名带 R/U-ID 注释（见 `src/lib/toast.test.ts`）。
- **IPC 边界（注入点）**：Tauri v2 的 `invoke` 经 `window.__TAURI_INTERNALS__` 路由；`Channel` 流由前端 `channel.onmessage(msg)` 驱动（`ipc.ts` 里 `new Channel<ProgressEvent>()` + `channel.onmessage = onEvent` 后 `invoke(cmd, { onEvent: channel })`）。**在 `__TAURI_INTERNALS__` 层拦截 invoke、直接调 `args.onEvent.onmessage(evt)` 即可回放事件流**，无需真后端。
- **命令契约（9 个）**：`generate_handler!`（`crates/gui/src/lib.rs`）= `scan_clean` · `clean` · `cancel_scan` · `analyze` · `classify_marked` · `delete_marked` · `check_fda` · `open_fda_settings` · `open_trash`。前端封装在 `crates/gui/frontend/src/lib/ipc.ts` 一一对应。
- **camel↔snake 漂移面**：Rust 签名用 snake（`confirm_token` / `on_event`），`ipc.ts` 传 camel（`confirmToken` / `onEvent`）。Tauri v2 默认按约定自动转换，故**当前可用**——但正是契约守卫要钉死的约定层。
- **macOS 原生 E2E 不可行**：`tauri-driver` 官方仅 Windows/Linux（macOS 无 WKWebView driver 工具）。→ 独立佐证「mock 边界、跳过原生」是唯一务实路径。

### Key Technical Decisions

- **KTD1｜Playwright + `page.addInitScript` 注入 mock**（非 tauri-driver）。`addInitScript` 在 app bootstrap 前把假 `window.__TAURI_INTERNALS__` 装入页面上下文，时序可靠；Playwright 驱动真实 Chromium 打开 `vite dev`（:1420）。
- **KTD2｜mock 挂在 IPC 边界（`__TAURI_INTERNALS__.invoke`），不 stub `ipc.ts`**。理由：若 stub `ipc.ts` 就绕过了真实 invoke 载荷，测不到命令名/参数形状，失去抓「按钮点不了」的意义。在边界拦截才能断言真实 invoke payload。
- **KTD3｜契约守卫 = 静态解析，跑在 vitest**。解析 `ipc.ts` 的 `invoke("name", {args})` 调用 ⇔ 解析 `lib.rs` 的 `generate_handler!` 与 `commands/*.rs` 的 `#[tauri::command]` 签名。轻、无需起后端。
- **KTD4｜夹具类型复用 `ipc.ts` 导出类型**（`ProgressEvent` 等）。夹具与被测契约同源，夹具漂移即 TS 报错。
- **KTD5｜冒烟门前置**：`svelte-check` + `vite build` 跑在 E2E 之前，最便宜的失败最先暴露。

### Alternatives Considered

- **WebdriverIO service（真原生，macOS 可用）** — 否决：会引入真后端 + 真删除风险、慢、flaky、维护成本高，与「跳过 Tauri、降低成本」目标背离。
- **vitest browser mode / `@testing-library/svelte`** — 更轻，但非真浏览器（用户明确要真浏览器 E2E），对布局/滚动保真弱。仅在契约守卫(U3)复用 vitest 作为 runner。
- **官方 `@tauri-apps/api/mocks` `mockIPC`** — 可用，主要面向进程内 runner；在 Playwright 页面上下文仍需经 `addInitScript` 注入，驱动 Channel 流不如自控灵活。作为 U1 实现参考，不作为主路径。

### Risks & Mitigations

- **mock 与真后端语义漂移**：契约守卫(U3)只钉签名/命令名，不覆盖语义。记为已知局限——语义漂移需后续轮的少量 Rust 命令单测或原生冒烟兜底（不在本轮）。
- **`__TAURI_INTERNALS__` 形状随 `@tauri-apps/api` 版本变**：固定依赖版本；U1 的骨架自测（canned invoke + Channel 回放）兜底探测。
- **Svelte 5 `$state` 异步渲染时序**：Playwright 一律用 auto-wait / `expect.poll` 断言，不用固定 sleep。

---

## Implementation Units

### U1. E2E 骨架 + Tauri mock 注入层

- **Goal:** 装好 Playwright、配 vite dev、实现 `addInitScript` 注入假 `__TAURI_INTERNALS__`（invoke 派发到可覆盖的夹具后端；流式命令经 `args.onEvent.onmessage` 回放）。
- **Requirements:** R1, R2, R5(部分)
- **Dependencies:** 无
- **Files:**
  - `crates/gui/frontend/playwright.config.ts`（webServer 起 `vite`，`url: http://localhost:1420`，`reuseExistingServer: !process.env.CI`，headless）
  - `crates/gui/frontend/e2e/support/tauri-mock.ts`（注入脚本 + 命令处理器注册表）
  - `crates/gui/frontend/package.json`（新增 `@playwright/test` devDep；`e2e` 脚本；冷环境首跑需 `npx playwright install chromium`，见 U7）
- **Approach:** `tauri-mock.ts` 导出 `installTauriMock(page, handlers)`：`page.addInitScript` 装 `window.__TAURI_INTERNALS__ = { invoke(cmd,args), transformCallback, ... }`。invoke 按 `cmd` 查处理器；流式命令的处理器拿到 `args`（含 `args.onEvent` Channel），逐条 `args.onEvent.onmessage(evt)` 回放后 resolve 最终结果。处理器可 per-test 覆盖。
  - **未注册命令契约（必须定死）**：`mock.invoke` 命中不到处理器时 **reject 并带可诊断信息**（`Unmocked command: <cmd>`），不静默返回 undefined——否则漏 mock 的调用会以难懂的下游报错出现。这条契约把「新增命令忘了 mock」暴露为清晰失败。
  - **必带的默认处理器**：除 9 个应用命令外，前端还经同一 `__TAURI_INTERNALS__.invoke` 走 Tauri **path 插件**——`ipc.ts` 的 `userHome()`→`homeDir()`→`invoke("plugin:path|resolve_directory", ...)`（`Analyze.svelte` 启动分析前 `await userHome()`）。故默认处理器集必须含 `plugin:path|resolve_directory`（返回假 home 路径），否则 U5 在 `analyze` 被调用前就断裂。
- **`reuseExistingServer` 理由**：`vite.config.ts` 为 `strictPort: true`（1420），若已有 dev server 占用会直接报错而非换端口；`reuseExistingServer` 复用既有 server 规避冷/热环境冲突。
- **Patterns to follow:** 复用现有 vitest 风格；TS strict。
- **Execution note:** 冒烟先行——先用一个 canned 命令 + 2 条 Channel 事件证明注入通路，再建各流 spec。
- **Test scenarios:**
  - 骨架自测：一个最小页面加载 → mock invoke 返回预设值 → 断言收到。
  - Channel 回放：注册一个流式处理器回放 2 条事件 → 断言 `onmessage` 依序收到两条。
- **Verification:** `npx playwright test e2e/support`（或骨架自测 spec）通过；无需真 Rust 后端。

### U2. 夹具事件流 + 后端夹具

- **Goal:** 提供可复用的真实规模事件流与各命令默认响应。
- **Requirements:** R2, R3
- **Dependencies:** U1
- **Files:** `crates/gui/frontend/e2e/support/fixtures.ts`
- **Approach:** 类型从 `../../src/lib/ipc` 导入（KTD4）。工厂：`scanStream({found: N, withRisky, error})` 产 `Scanning`→N×`Found`→`CategoryDone`→(`Complete`|`Error`)；`cleanStream(freed,count)` 产 `CleaningFile`×→`CleaningDone`；`analyzeStream()` 产 `Entry`×→`Progress`→`Finished`。命令默认响应：`check_fda`(authorized/unauthorized 两版)、`classify_marked`(Risky/Safe)、`clean`/`delete_marked`(CleanReport)、`plugin:path|resolve_directory`(返回假 home 路径，供 `userHome()`)。
- **默认前置**：`App.svelte` 挂载即 `runFdaCheck()`（invoke `check_fda`），U4/U5 须以 `check_fda`(authorized) 为默认才能到达主界面——夹具默认集须启用 authorized 版。
- **Patterns to follow:** `src/lib/aggregate.test.ts` 里已有的事件构造习惯。
- **Test scenarios:** `Test expectation: none -- 纯测试夹具，无独立行为（由消费它的 U4–U6 覆盖）。`
- **Verification:** 被 U4–U6 引用后类型检查通过、E2E 绿。

### U3. IPC 契约守卫

- **Goal:** 静态比对前端 invoke 调用 ⇔ Rust 命令注册表 + 签名，钉死命令名/参数/camel↔snake 约定。
- **Requirements:** R4
- **Dependencies:** 无（概念上属自测套件，可独立跑）
- **Files:** `crates/gui/frontend/e2e/contract.test.ts`（vitest）
- **Approach:** 读 `../../src/commands? no` → 读仓库源：`crates/gui/src/lib.rs` 提 `generate_handler!` 命令名；`crates/gui/src/commands/*.rs` 提每个 `#[tauri::command] fn` 的参数名（排除 `app: AppHandle` / `State`）；`crates/gui/frontend/src/lib/ipc.ts` 提 `invoke("<name>", { ... })` 的命令名与参数键。比对：命令集合相等；每个前端参数（camel→snake）都存在于对应 Rust 签名。
- **Patterns to follow:** vitest `describe/it`；用 `node:fs` 读源文件（相对路径以 repo 根为基）。
- **Test scenarios:**
  - `generate_handler!` 的 9 命令与 `ipc.ts` 调用集合**完全相等**（无缺失、无多余、无拼错）。
  - `clean` / `delete_marked` 的 `confirmToken`/`onEvent` 映射到 `confirm_token`/`on_event` 且存在于 Rust 签名。
  - 负向自证：喂一个「前端多调一个不存在命令」的构造样本 → 守卫报错（证明它真能红）。
  - `Covers R4.`
- **Verification:** `vitest run e2e/contract.test.ts` 绿；手动改一个 Rust 命令名可令其红。

### U4. Clean 流 E2E（主干 + 关键分支）

- **Goal:** 覆盖 Clean 全链的功能行为与关键分支。
- **Requirements:** R3, SC5
- **Dependencies:** U1, U2
- **Files:** `crates/gui/frontend/e2e/clean.spec.ts`
- **Approach:** 用 U2 夹具驱动 `scan_clean`；对每个动作断言 invoke 的命令名与参数 payload 精确匹配，并断言 UI 响应。
- **Patterns to follow:** `src/routes/Clean.svelte` 的按钮接线（`startScan`/`primaryDelete`/`doClean`/`cancel`）。
- **Execution note:** 先写「scan→render」一条失败测试打通夹具通路，再逐分支加。
- **Test scenarios:**
  - `scan_clean` 灌多条 `Found` → `StreamingList` 渲染对应项、`SummaryHeader` 合计正确。
  - 勾选若干 **Safe** 项 → 删除 → `ConfirmDelete` 出现（无口令）→ 确认 → 断言 `invoke("clean", { paths, confirmToken: "", onEvent })` 参数精确。
  - 含 **Risky** 项 → 确认弹窗要求 type-to-confirm；空/错口令 → 不放行；输入 `delete` → 放行 → `invoke("clean", { confirmToken: "delete", ... })`。`Covers SC5.`
  - `clean` 回放 `CleaningDone` → `CleanReceipt` 显示 freed/count；`UndoToast` 出现 → 点撤销 → `invoke("open_trash")`。
  - 扫描中点取消 → `invoke("cancel_scan")`；取消后列表不被旧结果污染（对齐 correctness review P2 注释）。
  - `scan_clean` 回放 `Error` 事件 → UI 显示错误态，不静默、不卡死。
- **Verification:** `clean.spec.ts` 全绿；每个 invoke 断言含命令名 + 参数。

### U5. Analyze 流 E2E

- **Goal:** 覆盖 Analyze 主干 + Risky 确认分支。
- **Requirements:** R3, SC5
- **Dependencies:** U1, U2
- **Files:** `crates/gui/frontend/e2e/analyze.spec.ts`
- **Approach:** 用 `analyzeStream` 夹具驱动 `analyze`；断言树渲染、标记、分级回查、删除的 invoke 参数。
- **Patterns to follow:** `src/routes/Analyze.svelte`、`src/lib/StreamingList.svelte`。
- **Test scenarios:**
  - `analyze` 回放 `Entry/Progress/Finished` → 树按体积渲染。
  - 标记路径 → `invoke("classify_marked", { paths })`；含 Risky → 确认弹窗三通道 + type-to-confirm。
  - 确认删除 → `invoke("delete_marked", { paths, confirmToken, onEvent })` 参数精确。
  - 切 Tab clean↔analyze 不串状态（`App.svelte` tab 切换）。
- **Verification:** `analyze.spec.ts` 全绿。

### U6. Onboarding / FDA 权限门 E2E

- **Goal:** 覆盖启动权限门三态与跳设置动作。
- **Requirements:** R3
- **Dependencies:** U1, U2
- **Files:** `crates/gui/frontend/e2e/onboarding.spec.ts`
- **Approach:** per-test 覆盖 `check_fda` 处理器为 authorized / unauthorized / reject 三态。
- **Patterns to follow:** `src/App.svelte` 的 `runFdaCheck` 分支、`src/routes/Onboarding.svelte`。
- **Test scenarios:**
  - `check_fda` authorized → 无 Onboarding，直达主界面 tab。
  - unauthorized → Onboarding 显示 probes；点跳转 → `invoke("open_fda_settings")`；`onRecheck` → 进主界面。
  - `check_fda` reject → 控制台警告 + 降级 `ready`（不卡在 checking 态）。
- **Verification:** `onboarding.spec.ts` 全绿。

### U7. 单命令自测入口 + 冒烟门

- **Goal:** 一条 `pnpm selftest` 串起冒烟 + 契约 + E2E，失败非零退出；文档化「Claude 交付 GUI 前必跑」。
- **Requirements:** R5, SC1
- **Dependencies:** U1–U6
- **Files:**
  - `crates/gui/frontend/package.json`（`selftest`: `svelte-check && vite build && vitest run && playwright test`；如需拆分保留 `e2e`/`contract` 子脚本；`postinstall` 或 selftest 前置 `npx playwright install chromium`，保证冷环境无人值守首跑不因缺浏览器二进制而假失败）
  - `crates/gui/frontend/README.md` 或根 `CLAUDE.md` 补一句自测入口（若不便改 `CLAUDE.md`，留 TODO 指向）
- **Approach:** 顺序串联，任一步非零即整体失败。契约守卫走已有 `vitest`，E2E 走 `playwright`。浏览器二进制经 `playwright install` 就位（冷环境一次性），避免「环境未就绪」污染 KD2 的 forcing-function 信号。
- **Test scenarios:** `Test expectation: none -- 编排脚本，无独立行为；验收由 Definition of Done 的「当前分支跑红」证明。`
- **Verification:** `pnpm selftest` 单命令跑完；人为破坏任一层可令其非零退出。

---

## Verification Contract

- **单命令**：`cd crates/gui/frontend && pnpm selftest` —— 无头、失败非零退出（SC1/R5）。
- **冒烟层**：`svelte-check` 0 error；`vite build` 成功（KTD5）。
- **契约层**：`contract.test.ts` 绿，且改一个 Rust 命令名/参数可令其红（SC4/R4）。
- **E2E 层**：U4–U6 主干 + 关键分支断言全绿，每个交互断言含 invoke 命令名 + 参数 + UI 响应（SC3/R3）。
- **交付纪律**：Claude 在声明「GUI 工作完成」前必跑 `selftest`，红则不得交付。

## Definition of Done

- `pnpm selftest` 单命令可跑、无头、失败非零退出。
- **骨架建成时在当前（未修）分支上跑红，复现骨架可探测类别（前端接线 / 命令契约）的现存故障至少一处**；若该类故障复现不出（真实故障属 WKWebView-only / 后端-only / 卡顿），如实记录并说明原因，不硬造测试（KD2 / 假设 A1 / SC2 判据限定）。
- 四条流主干 + 关键分支各有断言（命令名 + 参数 + UI 响应）。
- 契约守卫在命令名/参数漂移时红，且含一个负向自证样本。
- Risky type-to-confirm 覆盖：无 `delete` 口令不得删 Risky（SC5）。
- 自测入口在 `README`/`CLAUDE.md` 有一行指引（或明确 TODO）。

---

## 验收记录（DoD 结果 · 2026-07-08）

全部单元已实现，自测骨架建成并跑通：

- **冒烟门**：`svelte-check` 0 error；`vite build` 成功。
- **单测 + 契约**：`vitest` 7 文件 / 39 测试全绿（含 U3 契约守卫 6 条，带 `ghost_command` 负向自证）。
- **E2E**：Playwright 16 用例全绿（Clean 7 / Analyze 4 / Onboarding 3 / smoke 2），覆盖四条流主干 + 关键分支（Risky type-to-confirm、取消、Error、FDA 三态）。
- **gui crate**：`cargo check -p mc-gui` 通过（Tauri 后端能编译，排除「构建都过不了」）。

**KD2/SC2 forcing function —— 诚实结论：在骨架可探测类别（前端接线 / 命令契约 / serde 形状）内，当前分支未复现「按钮点不了」故障。** 依据：16 E2E 全绿 + 逐结构 serde 审计（`ProgressEvent`/`AnalyzeEvent`/`ScanResult`/`ScanItem`/`CategoryGroup`/`CleanReport`/`CleanedItem`/`DirNode`/`SafetyLevel`/`FdaStatus`/`ProbeResult`/`PathStatus` 与 `ipc.ts` **逐字段逐 tag 一致，含内部标签 `tag=status,content=detail`**）——即「mock 会不会骗人」这一 gap 在本分支不成立，mock 形状 = Rust 真实输出。故 SC2 走「该类故障复现不出、如实记录」的合法路径，而非硬造红。

**用户「按钮点不了 / 卡的要死」若仍存在，只可能落在骨架明确不覆盖的类别**：① WKWebView 特有运行时（Chrome 比 Safari 15.4 宽容，如 View Transitions 等 API 差异）；② 卡顿/性能（OQ1，本轮不测）；③ 抱怨早于本分支若干提交。→ 建议下一步：真机 `cargo tauri dev` 观察 WKWebView 行为，或启动 OQ1 性能档。

**包管理器：npm → pnpm（用户要求，2026-07-08）**。`package-lock.json` 删除，生成 `pnpm-lock.yaml` + `pnpm-workspace.yaml`（`allowBuilds: esbuild: true`，让 esbuild postinstall 正常跑）。`selftest` 脚本链、`playwright.config.ts` 的 `webServer.command`、README 命令全部改 `pnpm`。**pnpm 下复验**：`pnpm check` 0 error / `pnpm build` 成功 / `pnpm test` 39/39 / `pnpm e2e` 16/16。

**唯一未在 agent shell 跑通的一步：`pnpm e2e:install`（下 170MB 自带 chromium）**。现象：下载进程活着但**无网络 socket、无临时文件、8 分钟零字节**（`lsof` 证实）。根因定位：`cdn.playwright.dev` 的旧 `dbazure` 路径撞公司云壳域名拦截（返回中文拦截页），而现用的 cft 路径（`/builds/cft/.../chrome-mac-arm64.zip`）小范围探测能秒回真 ZIP、但整段 170MB 流经云壳被卡死。**这台机器有外网**（其它进程连着阿里云 IP），**姊妹项目 `claude-devtools-rs/ui` 的 `playwright-report/` 证明用户正常终端能下能跑**。→ 结论：标准范式（自带 chromium）配置正确，用户**在正常终端**跑 `pnpm selftest` 即可下好浏览器跑通全套；agent Bash 这条下载路走不通是 shell/云壳限制，非配置问题。逃生口 `PW_CHANNEL=chrome`（系统 Chrome，免下载）+ `PW_NO_WEBSERVER=1`（受限 shell 跳过托管 webServer）保留为可选环境变量，本 shell 的 16/16 即经此逃生口跑出。

**Playwright 托管 `webServer` 在受限 shell 静默空跑**（exit 0 / 0 用例 / 0 输出），即便 vite 已起 + `reuseExistingServer`；正常终端无此问题，故 `PW_NO_WEBSERVER` 仅受限 shell 需要。

---

## Outstanding Questions（延后，不阻塞本轮）

- **OQ1｜卡顿阈值**：多少响应时间 / 帧预算算「不卡」？性能档为后续轮，需先在此骨架上建基准再定门槛。
- **OQ4｜语义漂移兜底**：契约守卫只钉签名；后续是否补少量 Rust 命令单测或一次性原生冒烟以覆盖 mock 与真后端的语义差？
