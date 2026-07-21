---
title: "feat: 权限跳过区内联 FDA 授权引导"
date: 2026-07-21
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
product_contract_source: ce-plan-bootstrap
origin: docs/plans/2026-07-20-002-feat-gui-permission-skip-parity-plan.md（Deferred to Follow-Up Work 第 3 项「授权引导深化」）
depth: Lightweight
---

# feat: 权限跳过区内联 FDA 授权引导

**Product Contract preservation:** 无独立需求文档（solo 直接规划，`product_contract_source: ce-plan-bootstrap`）；产品意图取自计划 002 的 Deferred 第 3 项。不改动任何既有产品范围、安全语义或删除信任链——纯前端在既有跳过展示块内增加一个跳转按钮。

---

## Summary

计划 002 让五入口（Clean / Purge / Analyze / Orphans / Uninstall）都会展示「因权限跳过 N 项」，但只做**只读展示**——用户看到"扫描漏了这些受保护路径"，却无法就地修复。本轮在跳过展示块内加一个「打开磁盘访问权限设置」按钮，复用已有的 `openFdaSettings` IPC 命令（Onboarding 与命令面板 `act.fda` 已在用），把"看到跳过了什么"闭合成"一步去授权"。

范围严格限定呈现层：无 core 改动、无新命令、无安全语义变更。

---

## Problem Frame

**现状**：跳过展示块（三入口经 `SkippedNoPermission.svelte`，Clean/Purge 各自内联同构块）只渲染路径列表 + 折叠 toggle。用户理解了"因未授权 FDA 跳过了这些"，但要去授权得自己知道「系统设置 → 隐私与安全性 → 完全磁盘访问」这条路径，或退回 Onboarding。

**缺口**：`openFdaSettings` 命令、`FDA_SETTINGS_URL`、命令面板 `act.fda` 动作全都存在（`crates/gui/src/commands/permission.rs:36`、`App.svelte:42`），Onboarding 首屏也在用（`Onboarding.svelte:68`）。跳过区是用户**最需要**这个动作的语境，却偏偏没接线。

**为什么现在做**：这是计划 002 显式列出的 Deferred 项，前置（`openFdaSettings` 通电、五入口跳过展示对齐）已全部满足。纯接线，零新算法、零后端。

---

## Requirements

- **R1** 五入口的跳过展示块内均出现「打开磁盘访问权限设置」按钮，点击调用 `openFdaSettings`。
- **R2** 按钮仅在有跳过项时出现（跟随现有 `skipped.length > 0` 渲染条件），无跳过项时不显示。
- **R3** 不改安全模型：按钮只跳转系统设置，不触碰 `selected` / `marked` / 删除授权路径；跳过项仍是只读展示，永不进待删集（继承计划 002 R5 结构性保证）。
- **R4** 提示授权后通常需重启 app 生效（与 Onboarding 一致的用户预期）。
- **R5** 五入口行为/文案/样式一致——共享组件与两处内联块视觉与交互同构。

---

## Key Technical Decisions

**KTD1 — 复用 `openFdaSettings`，不新增命令。**
`crates/gui/frontend/src/lib/ipc.ts:326` 已导出 `openFdaSettings(): Promise<void>`，后端 `open_fda_settings` 已注册（`lib.rs:94`）。前端三入口 import 该函数并绑到按钮 onclick 即可。
_代价_：无。_收益_：零后端改动，行为与 Onboarding/命令面板完全一致。

**KTD2 — 共享组件加按钮覆盖三入口；Clean/Purge 内联块各自加同构按钮。**
计划 002 KTD4 把 Clean/Purge 保留为内联块（相位机不同、组件化留 Deferred），本轮**不推翻该边界**——不做组件化合并。故按钮要落三处：`SkippedNoPermission.svelte`（覆盖 Analyze/Orphans/Uninstall）+ Clean 内联块 + Purge 内联块。
_代价_：两处内联块重复少量 markup/样式（约 3 行按钮 + 复用现有 `.link` 样式）。_收益_：不引入本轮不该做的组件化重构，风险面收敛在"加一个按钮"。
_备选_：借机把 Clean/Purge 迁到共享组件——**否决**，超出本轮范围，属计划 002 已 Deferred 的独立项。

**KTD3 — 按钮样式复用现有 `.link` 类，不新增视觉语言。**
三处跳过块已有 `.link`（accent 色文字按钮）用于展开/收起 toggle。FDA 按钮沿用同类样式，与 toggle 并列，保持跳过区视觉一致、不引入新按钮层级。

---

## Implementation Units

### U1. 共享组件 `SkippedNoPermission.svelte` 加 FDA 引导按钮

**Goal:** 让 Analyze / Orphans / Uninstall 三入口的跳过块出现「打开磁盘访问权限设置」按钮。

**Requirements:** R1, R2, R4, R5

**Dependencies:** 无

**Files:**
- `crates/gui/frontend/src/lib/SkippedNoPermission.svelte`（修改）
- `crates/gui/frontend/src/lib/ipc.ts`（确认 `openFdaSettings` 已导出，无需改）

**Approach:**
- import `openFdaSettings`（`from "./ipc"`）。
- 在展开/收起 toggle 旁（或列表下方）加一个 `.link` 按钮，文案「打开磁盘访问权限设置」，onclick 调 `void openFdaSettings()`。
- 加一句轻量提示文案（授权后需重启 app 生效），与 Onboarding 措辞对齐；可作为按钮下方的小字或 title。
- 按钮在 `{#if skipped.length > 0}` 块内，天然满足 R2。

**Patterns to follow:** 组件内现有 `.link` 按钮（展开/收起）；`Onboarding.svelte:68` 的 `openFdaSettings` 绑定与重启提示措辞。

**Test scenarios:**
- Covers R1/R2. 组件渲染测试：`skipped` 非空 → 按钮出现；`skipped` 为空 → 整块（含按钮）不渲染。
- Covers R1. 点击按钮调用 `openFdaSettings`（mock IPC，断言被调用一次）。
- 展开/收起 toggle 与 FDA 按钮共存、互不干扰。

**Verification:** `pnpm test`（组件测试）通过；三入口手动/e2e 冒烟见 U3。

### U2. Clean / Purge 内联跳过块加同构 FDA 按钮

**Goal:** Clean 与 Purge 的内联跳过块出现与共享组件同款按钮，五入口一致。

**Requirements:** R1, R2, R4, R5

**Dependencies:** U1（确定按钮文案/样式/提示后在内联块复制同构实现）

**Files:**
- `crates/gui/frontend/src/routes/Clean.svelte`（修改，内联块约 `:306-317`）
- `crates/gui/frontend/src/routes/Purge.svelte`（修改，内联块约 `:366-378`）

**Approach:**
- 两文件已 import 或可 import `openFdaSettings`（Clean 未 import 则加）。
- 在内联 `.skipped` 块的 toggle 旁加同 U1 的 `.link` 按钮 + 提示文案。
- 复用各文件已有的 `.skipped` / `.link` 样式，不新增 CSS（若样式缺 `.link` 则补最小定义，与共享组件一致）。

**Patterns to follow:** U1 的按钮实现；各文件既有内联 `.skipped` 块结构（计划 002 建立的同构块）。

**Test scenarios:**
- Covers R5. Clean/Purge 内联块渲染测试或 e2e：有跳过项时按钮出现、点击调 `openFdaSettings`。
- 与 U1 视觉/行为一致性（人工比对文案与样式）。

**Verification:** `cargo build`（前端不影响 Rust 编译，但走 workspace 构建确认无回归）；`pnpm test` 通过。

### U3. e2e 冒烟——五入口跳过区 FDA 按钮可见且可点

**Goal:** 结构性保证五入口跳过区都能触达 FDA 授权，且按钮不污染删除信任链。

**Requirements:** R1, R3, R5

**Dependencies:** U1, U2

**Files:**
- `crates/gui/frontend/tests/`（复用计划 002 的跳过展示 e2e；扩展或新增用例）—— 执行时核实现有跳过展示 e2e 文件路径与命名，在其基础上加断言而非另起。

**Approach:**
- 在既有"权限跳过展示"e2e（计划 002 建立）里，对能构造跳过态的入口加断言：FDA 按钮存在、可点、点击触发 `openFdaSettings`（mock/桩）。
- 断言按钮**不**改变待删集/勾选态——纯跳转，跳过项仍不可选（继承计划 002 R5 的结构性断言，本轮不放松）。

**Execution note:** 复用计划 002 已有的跳过展示 e2e 骨架；本单元是加断言，不是重建测试。若 e2e 沙箱 webServer 有超时问题，按既有 workaround（手动起 dev + `PW_NO_WEBSERVER=1`）跑。

**Test scenarios:**
- Covers R1/R5. 至少一个共享组件入口 + 一个内联入口（如 Orphans + Clean）：跳过态下 FDA 按钮可见、点击调 `openFdaSettings`。
- Covers R3. 断言点击 FDA 按钮后跳过项仍不在 `selected`/`marked`，删除授权取项不返回跳过路径。

**Verification:** e2e 套件通过；`cargo test` + `pnpm test` 全绿。

---

## Assumptions

- `openFdaSettings` 的当前行为（打开系统设置 URL + 前端提示重启）满足本轮需求，无需为跳过区语境定制。若真机反馈需更精准的深链，属后续。
- Clean/Purge 内联块保持内联（不组件化）——沿用计划 002 KTD4 边界；本轮不触发组件化重构。

---

## Scope Boundaries

### 不做（Deferred to Follow-Up Work）
- **跳过展示组件化**：Clean/Purge 内联块合并进共享组件仍是计划 002 已 Deferred 的独立项，本轮不做。
- **FDA 深链精化**：按钮跳转粒度沿用现有 `FDA_SETTINGS_URL`；若需直达具体面板/预填 app 属后续。
- **CLI 侧授权引导**：CLI 权限 UX 归 `mc doctor`（计划 002 KTD3），本轮不涉及。
- **授权后自动重扫**：点 FDA 按钮 → 授权 → 自动回来重扫的闭环，属后续体验优化，本轮只做跳转 + 提示。

### 非目标（Outside identity）
- 编程申请 FDA（TCC 不弹框、非公开 API，`permission.rs` 已注明不可行）。

---

## Risks & Dependencies

| 风险 | 缓解 |
|---|---|
| 按钮误接进删除路径（违反 R3） | 按钮只调 `openFdaSettings`，不碰 `selected`/`marked`；U3 断言跳过项不可选、授权取项不返回跳过路径。 |
| 五入口文案/样式漂移（违反 R5） | U1 先定型，U2 复制同构实现；U3 人工比对 + e2e。 |
| 内联块缺 `.link` 样式导致按钮裸样式 | U2 执行时确认各文件 `.skipped`/`.link` 样式，缺则补最小定义与共享组件一致。 |

---

## Sources & Research

- `docs/plans/2026-07-20-002-feat-gui-permission-skip-parity-plan.md` —— 本计划的 origin（Deferred 第 3 项）。
- `crates/gui/frontend/src/lib/SkippedNoPermission.svelte` —— 共享跳过展示组件（三入口）。
- `crates/gui/frontend/src/routes/Clean.svelte:306-317` / `Purge.svelte:366-378` —— Clean/Purge 内联跳过块。
- `crates/gui/src/commands/permission.rs:36` / `crates/gui/frontend/src/lib/ipc.ts:326` —— `openFdaSettings` 命令与前端绑定。
- `crates/gui/frontend/src/routes/Onboarding.svelte:68` —— 既有 FDA 按钮用法参照。
