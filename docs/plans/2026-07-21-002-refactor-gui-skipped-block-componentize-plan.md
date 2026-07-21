---
title: "refactor: Clean/Purge 权限跳过块复用 SkippedNoPermission 组件"
date: 2026-07-21
type: refactor
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
product_contract_source: ce-plan-bootstrap
depth: lightweight
---

# refactor: Clean/Purge 权限跳过块复用 SkippedNoPermission 组件

## Summary

Clean 与 Purge 两个入口的「因权限跳过 N 项」展示块目前是**内联手写**的，与共享组件 `crates/gui/frontend/src/lib/SkippedNoPermission.svelte` 逐行同构（同 toggle、同列表、同 FDA 引导按钮、同 CSS）。Analyze/Orphans/Uninstall 三入口已复用该组件；本计划把 Clean/Purge 也切到共享组件，消除五入口中最后两处重复，让「跳过展示」真正单点定义。

这是历次计划（020/021/026/027/028 及 002 KTD4、skip-fda-guide）反复列入 `Deferred to Follow-Up Work` 的「跳过展示组件化」——其触发条件（"若重复度证明值得，后续抽公共组件"）现已满足：五入口刚完成行为/样式对齐，Clean/Purge 内联块与组件已证实逐行同构。

**行为零变更**：这是纯结构性重构。渲染输出、DOM 结构、文案、e2e 选择器（role+name）全部不变。

---

## Problem Frame

**现状**：`SkippedNoPermission.svelte`（`crates/gui/frontend/src/lib/`）是三入口共享的跳过展示组件。但 Clean（`routes/Clean.svelte:307-326` + 样式 `493-520`）和 Purge（`routes/Purge.svelte:367-386` + 样式 `561-588`）仍各自维护一份内联同构块。

**痛点**：同一段展示逻辑存在三份定义（组件 1 份 + 内联 2 份）。任何跳过区的改动（如上一轮的 FDA 引导按钮）都要在多处同步，`skip-fda-guide` 计划的 System-Wide 教训正是"五入口文案/样式漂移"风险，靠人工比对 + e2e 兜底。组件化把这条风险从根上消除。

**为何现在做**：五入口刚对齐（PR #59/#60），三份实现确证同构，是抽公共载体的最低成本时机——晚做则每轮跳过区演进都要付多处同步税。

---

## Scope Boundaries

### 本轮做
- Clean 内联跳过块 → `<SkippedNoPermission {skipped} />`
- Purge 内联跳过块 → `<SkippedNoPermission {skipped} />`
- 删除 Clean/Purge 中仅服务跳过块的 CSS 与 `openFdaSettings` 导入（若无其他引用）

### 不做（Deferred to Follow-Up Work）
- **组件 API 扩展**：当前组件 props 仅 `skipped: string[]`，足够 Clean/Purge。不为本轮引入额外参数。
- **`skipped` 状态管理统一**：Clean/Purge 从 `ProgressEvent` 流累积 `skipped`，与三入口的取值时机不同（事件 vs 返回值）。本轮只替换**展示块**，不动各入口的 `skipped` 采集逻辑。

### 非目标（产品身份边界）
- 不改跳过项的只读语义（永不进待删集）、不碰 `selected`/`marked`、不改删除信任链。
- 不改 FDA 引导按钮行为（仍只调 `openFdaSettings`）。
- 不改 core/CLI/TUI 任何代码——纯前端 Svelte 重构。

---

## Key Technical Decisions

### KTD1 — 保留外层 `phase` 守卫，内层换组件

组件内部已有 `{#if skipped.length > 0}` 守卫，但 Clean/Purge 内联块外还包了 `{#if phase === "results" && skipped.length > 0}`。替换时**保留外层 `phase === "results"` 条件**，内层直接放 `<SkippedNoPermission {skipped} />`（组件自带的 length 守卫与外层 `&& skipped.length > 0` 冗余但无害；可简化为 `{#if phase === "results"}` 让组件自己判空——见 U1 待实现决策）。理由：`phase` 是入口特有的相位坐标，不属于组件职责；组件只管"给我 skipped 列表，我负责展示"。

### KTD2 — 死样式一并删除（含 `.link`，已核实无其他引用）

`.skipped`/`.skipped-list`/`.skipped-guide`/`.skipped-hint` 是跳过块专用，替换后删除。`.link` 类经核实在 Clean/Purge 两文件中**仅被跳过块的两个按钮使用**（Clean `:309`/`:320`、Purge `:369`/`:380`，均在内联块内），且共享组件 `SkippedNoPermission.svelte:38-46` 自带 `.link` 样式——故替换后两文件的 `.link` 定义（Clean `:484-492`、Purge `:552-560`）成为孤立 unused CSS，应一并删除。理由：留着会让 `svelte-check` 报 "Unused CSS selector .link"（warning 不 fatal，但引入噪声）。实现时仍以 `rg 'class="link"' <file>` 复核一次为准，若发现新增引用则保留。

---

## Implementation Units

### U1. Clean.svelte 切换到共享组件

**Goal**: Clean 入口的内联跳过块替换为 `<SkippedNoPermission {skipped} />`，删除随之死掉的样式与导入。

**Files**:
- `crates/gui/frontend/src/routes/Clean.svelte`（修改）
- `crates/gui/frontend/e2e/clean.spec.ts`（验证不改，跑通即可）

**Approach**:
1. 顶部 `import SkippedNoPermission from "../lib/SkippedNoPermission.svelte";`（参照 Orphans.svelte:30）。
2. 模板 `307-326` 的整个 `<div class="skipped">…</div>` 块替换为 `<SkippedNoPermission {skipped} />`，保留外层 `{#if phase === "results" ...}`（见 KTD1）。
3. 删除 `<style>` 中 `.skipped`/`.skipped-list`/`.skipped-guide`/`.skipped-hint`（`493-520` 区段）。`.link` 按 KTD2 先查后删。
4. 若 `openFdaSettings` 在 Clean 中已无其他调用点，从 `ipc` 导入（`Clean.svelte:9`）中移除；`showSkipped` 状态（`:41`）若无他用一并删除。
5. `skipped` 状态（`:40`）保留——它由流事件累积（`:109`），传给组件。

**待实现决策**: 外层守卫是保留 `&& skipped.length > 0` 还是简化为纯 `phase === "results"` 让组件判空——实现时取更简洁且 e2e 通过的写法。

**Patterns to follow**: Orphans.svelte / Analyze.svelte / Uninstall.svelte 已有的 `<SkippedNoPermission {skipped} />` 用法。

**Execution note**: 重构前先跑 `clean.spec.ts` 确立绿基线；替换后重跑，输出应逐字不变（characterization：e2e 即为行为契约）。

**Test scenarios**:
- 复用现有 `clean.spec.ts:38`「clean 内联跳过区『打开磁盘访问权限设置』触发 open_fda_settings」——重构后必须仍绿（选择器 role+name 不变）。
- 复用「因权限跳过 N 项」折叠区展开/收起断言（`clean.spec.ts:51`）——仍绿。
- Test expectation: 不新增测试——现有 e2e 已是行为契约，重构以"测试逐字不变仍通过"为完成信号。

**Verification**: `clean.spec.ts` 全绿；`svelte-check`/`tsc` 无未使用导入告警；肉眼确认无残留死样式。

### U2. Purge.svelte 切换到共享组件

**Goal**: 同 U1，作用于 Purge 入口。

**Dependencies**: 无硬依赖，但建议在 U1 之后做（U1 确立替换范式，U2 复制同构操作）。

**Files**:
- `crates/gui/frontend/src/routes/Purge.svelte`（修改）
- `crates/gui/frontend/e2e/purge.spec.ts`（验证不改，跑通即可）

**Approach**:
1. 顶部导入 `SkippedNoPermission`。
2. 模板 `367-386` 的 `<div class="skipped">…</div>` 替换为 `<SkippedNoPermission {skipped} />`，保留外层 `{#if phase === "results" ...}`。
3. 删除样式 `561-588` 区段的跳过块专用类；`.link` 按 KTD2 先查后删（Purge 可能有其他 link 按钮，尤其注意）。
4. `openFdaSettings` 导入（`Purge.svelte:17`）、`showSkipped`（`:51`）若无他用则删除。
5. `skipped`（`:50`）保留，由流事件累积（`:166`）传给组件。注意 Purge 有两处 `skipped = []` 重置（`:102`/`:152`）——保留不动。

**Patterns to follow**: U1 的替换范式。

**Execution note**: 同 U1，e2e 绿基线 → 替换 → 重跑逐字不变。

**Test scenarios**:
- 复用 `purge.spec.ts` 中跳过区 FDA 引导与折叠展开断言——重构后必须仍绿。
- Test expectation: 不新增测试——现有 e2e 为行为契约。

**Verification**: `purge.spec.ts` 全绿；`svelte-check`（`pnpm check`）无未使用导入/CSS 告警；无残留死样式。

---

## Verification Contract

- **门禁 1**：`crates/gui/frontend` 下 `pnpm e2e`（本仓统一 pnpm；单文件可 `pnpm e2e clean.spec.ts purge.spec.ts`）——clean.spec.ts + purge.spec.ts + 其余入口 spec 全绿。跳过区选择器为 role+name，重构对其透明。
- **门禁 2**：`svelte-check` / `tsc` 无新增告警，尤其无「未使用的导入/变量」（`openFdaSettings`/`showSkipped` 删净）。
- **门禁 3**：`cargo build -p mc-gui`（若前端产物参与构建）通过——本计划不碰 Rust，此为回归确认。
- **人工确认**：五入口跳过区渲染一致（Clean/Purge 现在与三入口共用同一组件，天然一致）。

---

## Definition of Done

- Clean.svelte 与 Purge.svelte 均以 `<SkippedNoPermission {skipped} />` 渲染跳过区，无内联同构块残留。
- Clean/Purge 中仅服务跳过块的样式与导入已删除（`.link` 等共用类按 KTD2 保守保留）。
- clean/purge e2e 全绿，输出与重构前逐字一致。
- 无 lint/type 告警，无 Rust 侧改动。

---

## System-Wide Impact

- **影响面**：仅 `crates/gui/frontend/src/routes/Clean.svelte` 与 `Purge.svelte` 两文件。
- **零行为变更**：跳过区 DOM/文案/交互不变；`SkippedNoPermission.svelte` 组件本身不改。
- **收益**：跳过展示从「组件 1 份 + 内联 2 份」收敛为单点定义，消除后续跳过区演进的多处同步税与漂移风险（skip-fda-guide 计划记录的 R5 风险从此由类型系统 + 单组件保证，而非人工比对）。

---

## Sources & Research

- `crates/gui/frontend/src/lib/SkippedNoPermission.svelte` —— 目标共享组件（三入口已复用）。
- `crates/gui/frontend/src/routes/Clean.svelte:307-326`（内联块）/ `493-520`（样式）—— U1 替换点。
- `crates/gui/frontend/src/routes/Purge.svelte:367-386`（内联块）/ `561-588`（样式）—— U2 替换点。
- `crates/gui/frontend/src/routes/Orphans.svelte:30` —— 组件复用范式参照。
- `crates/gui/frontend/e2e/clean.spec.ts:38,51` / `purge.spec.ts` —— 行为契约（重构后必须逐字通过）。
- `docs/plans/2026-07-20-002-feat-gui-permission-skip-parity-plan.md`（KTD4）—— 组件化 Deferred 的原始出处与触发条件。
- `docs/plans/2026-07-21-001-feat-gui-skip-fda-guide-plan.md` —— 上一轮五入口对齐，Deferred 第 1 项「跳过展示组件化」的直接前置。
