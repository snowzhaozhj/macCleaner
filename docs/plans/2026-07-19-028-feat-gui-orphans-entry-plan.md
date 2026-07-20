---
title: "feat: GUI 孤儿残留扫描入口（反向卸载）"
date: 2026-07-19
type: feat
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
product_contract_source: ce-plan-bootstrap
depth: standard
origin: docs/plans/2026-07-19-027-feat-orphan-leftover-scan-plan.md（Deferred to Follow-Up Work 第 1 项「GUI / TUI 孤儿扫描入口」）；docs/ideation/2026-07-07-next-step-tui-vs-gui.md（#1 GUI 主线）
---

# feat: GUI 孤儿残留扫描入口（反向卸载）

## Summary

把已出货的核心引擎 `Engine::scan_orphans()`（反向卸载：扫 `~/Library` 找父 App 已不存在的孤儿残留）接入 GUI，补齐 027 计划显式延后的「GUI / TUI 孤儿扫描入口」——core + CLI 已通电，GUI 是唯一缺口。新增第五个功能 tab「孤儿残留」：进入即扫描 → 逐项审查（**一律不预选**）→ 勾选 → 移废纸篓删除 → 回执 + 一键撤销。全程复用现有安全语义与组件，不重造引擎、不复制清理逻辑。

核心事实（已核实代码）：

1. **核心已就绪且同步**：`Engine::scan_orphans() -> Vec<ScanItem>`（`crates/core/src/engine.rs:48`）平价委托 `AppResolver::scan_orphans()`（`crates/core/src/app_resolver.rs:365`）。**同步、非流式、`#[must_use]`**，内部 `list_apps()` + 遍历 `~/Library`，可能数秒。fail-closed：已装集合为空时返回空（宁漏报不误杀）。
2. **孤儿一律不预选**：`scan_orphans` 对每一项（含 Safe）设 `preselect=false`（`app_resolver.rs:365` 文档注释）——工具主动发现、非用户点名，故永不默认删、永不自动删，须逐项手动勾。**这是与 clean/purge 的关键差异，GUI 必须原样继承。**
3. **GUI 命令层模板清晰**：`crates/gui/src/commands/` 一能力一文件。`clean.rs`（单阶段扫→删 + 写账本 + `CleanResponse{report, run_id}`）与 `uninstall.rs`（残留合成 `ScanResult` + 前端按各项自身 category 重新分组）合起来正是本命令的模板。授权闸 `authorize_deletion`、取项 `select_by_paths`、`CleanResponse`、`CONFIRM_TOKEN` 均已在 `commands/mod.rs` 沉淀，直接复用。
4. **前端路由/组件模板齐备**：`routes/Uninstall.svelte`（相位机 + StreamingList 动态类目 + ConfirmDelete 信任链 + CleanReceipt + UndoToast + 路由级 palette 命令）是最贴近的模板。`App.svelte:13` 的 `Tab` 联合类型 + `TAB_LABELS` + 导航按钮 + 静态 palette 命令是 tab 接线的四个改点。
5. **状态槽隔离约定**：`AppState`（`crates/gui/src/lib.rs:18`）已有 `last_scan`/`last_purge`/`last_uninstall`/`last_analyze` 四个独立槽（KTD：隔离防交替扫描误取）。孤儿需第五个独立槽 `last_orphans`。
6. **IPC 绑定 + 注册两处对齐**：`frontend/src/lib/ipc.ts` 一命令一 `invoke` 包装；`lib.rs:71` `generate_handler!` 宏登记全部命令。新命令两处都要补。

本计划：GUI 后端加 `commands/orphans.rs`（`scan_orphans` spawn_blocking 查询 + `clean_orphans` 移废纸篓删 + `last_orphans` 槽）；前端加 `routes/Orphans.svelte`（单阶段相位机）+ ipc 绑定 + 第五 tab 接线 + e2e。无新 core 改动、无 FFI、无 `unsafe`、不改删除授权安全承诺（含 Risky 二次校验、恒移废纸篓、无永久删除）。

**Product Contract preservation:** 无独立需求文档（solo 直接规划，`product_contract_source: ce-plan-bootstrap`）；产品意图取自 027 计划的 Deferred 条目与 07-07 ideation 的 GUI 主线，未改动任何既有产品范围。孤儿场景「不预选」语义原样继承核心引擎，不在 GUI 层重新解释。

---

## Problem Frame

孤儿残留（反向卸载）能力目前只有 CLI（`mc orphans`）和 core 引擎两层，GUI 用户（STRATEGY.md 的二次用户——普通 Mac 用户，正是最可能"装了又删应用留下残留"的群体）触达不到。027 计划把 GUI 入口显式列为 Deferred，前置条件（core `scan_orphans` 通电）现已满足。缺口纯在 GUI 接线：后端 Tauri 命令 + 前端路由/tab，无新算法。

**范围边界**：仅接 `~/Library`（用户级）孤儿的**扫描 + 审查 + 移废纸篓删除**闭环，完整继承核心的 fail-closed、不预选、分级、授权语义。

---

## Requirements

- **R1**：GUI 新增「孤儿残留」功能入口（第五 tab + palette 导航命令），进入即触发一次 `scan_orphans` 扫描。
- **R2**：孤儿列表**每一项默认不勾选**（继承 core `preselect=false`）——用户不主动勾选任何项时，删除按钮无可删目标，绝不出现"一键清空"。
- **R3**：扫描结果按各项自身 category（如「应用残留 (Caches)」）分组展示，附体积与证据文案（复用 `aggregateByCategory` + StreamingList + EvidenceCard）。
- **R4**：删除恒走 `DeleteMode::Trash`（GUI 无永久删除路径）；待删项从后端 `last_orphans` 槽按路径精确取出（不接受前端回传完整 `ScanItem`）；含 Risky 时后端二次校验确认口令（复用 `authorize_deletion`）——虽然 core 保证孤儿不产 Risky，仍保留闸做纵深防御。
- **R5**：删除成功后写账本并回传 `run_id`，前端展示回执 + 一键撤销（复用 `CleanResponse` + `UndoToast`，`HistoryCommand` 复用 `Clean`——见 KTD4）。
- **R6**：空扫描（无孤儿）、扫描失败、扫描线程异常各有明确 UI 态，不出现空白/冻结屏。
- **R7**：`last_orphans` 与其余四槽隔离——切 tab 或交替扫描时，孤儿删除不会误取 clean/purge/uninstall 结果的项。

---

## High-Level Technical Design

孤儿命令是 clean（单阶段扫→删→写账本）与 uninstall（残留合成 `ScanResult` + 前端重分组）的**交集**，但扫描端更简单（同步、非流式、无取消）。数据流：

```
[Orphans.svelte 进入]
      │ scanOrphans()               (ipc.ts)
      ▼
[commands::orphans::scan_orphans]   #[tauri::command] async
      │ spawn_blocking              (纯查询，数秒，无取消——同 scan_uninstall)
      ▼
  Engine::scan_orphans() -> Vec<ScanItem>   (core，同步，preselect=false)
      │ ScanResult::from_categories(单组)
      │ 存 last_orphans 槽
      ▼
[前端] aggregateByCategory 重分组 → StreamingList 渲染（全部未勾选）
      │ 用户逐项勾选 → ConfirmDelete（含 Risky 才要求口令）
      ▼
[commands::orphans::clean_orphans]  #[tauri::command] async
      │ spawn_blocking：短临界区 clone 待删项 → drop 锁
      │ authorize_deletion（纵深防御）
      │ Engine::clean(refs, Trash, reporter)   (流式进度经 Channel)
      │ history::record_run → run_id
      ▼
CleanResponse{report, run_id} → CleanReceipt + UndoToast（一键撤销）
```

**与两个模板的差异（为何不能纯照抄任一个）：**

| 维度 | clean/purge | uninstall | **orphans（本计划）** |
|---|---|---|---|
| 扫描端 | 流式 + 可取消 | 同步 list（阶段一） | **同步一次性，无取消**（同 scan_uninstall 端） |
| 阶段 | 单阶段 | 两阶段（选 App → 解析残留） | **单阶段**（进入即全局扫） |
| 预选 | 按规则 preselect | 按规则 preselect | **恒不预选**（核心强制，R2） |
| 状态槽 | last_scan / last_purge | last_uninstall | **last_orphans（新）** |

前端相位机（较 uninstall 九态大幅简化）：`loading → ready | empty | error → deleting → done`。

---

## Key Technical Decisions

**KTD1：扫描端走 `spawn_blocking` 同步查询，不引进度 Channel，不引取消。**
core `scan_orphans` 同步返回全量 `Vec<ScanItem>`，无 emit 事件；镜像 `scan_uninstall`（`commands/uninstall.rs:27`）——纯查询、可能数秒、放 `spawn_blocking` 避免冻结 async 运行时，但不装 `begin_operation` 取消 flag（无中途可取消的流式扫描）。删除端 `clean_orphans` 才用 Channel + `begin_operation`（与 clean/purge 一致）。
_理由_：孤儿扫描是一次性快照，无增量流可展示进度或取消；强行套 clean 的流式脚手架是无谓复杂度。

**KTD2：孤儿"不预选"由核心保证，GUI 不额外处理，但 UI 文案须显式提示。**
`scan_orphans` 已对每项设 `preselect=false`，前端 `LiveItem.selected` 直接映射，天然全部未勾。GUI 不写任何"清空预选"特判（那会与核心语义双写漂移）。但需在列表头/空选态加一句文案说明"孤儿残留需手动勾选要回收的项"，避免用户误以为列表是坏的。
_理由_：Core Principle——预选语义单一来源在核心；GUI 只消费不重解释（呼应 [[forcing-one-trust-axis-misses-sibling-axis]] 的教训：外部/核心数据的每个属性都别在消费面重新判定）。

**KTD3：新增 `last_orphans: Arc<Mutex<Option<ScanResult>>>` 独立槽。**
在 `AppState`（`lib.rs:18`）加第五槽，`Default` 初始化 `None`。删除时从此槽按路径 `select_by_paths` 取项。
_理由_：R7 隔离——与 last_purge/last_uninstall 独立槽同款约定，防交替扫描误取（027 与 GUI 既有计划反复确立的模式）。

**KTD4：删除写账本复用 `HistoryCommand::Clean`，不新增 orphans 变体。**
`clean_orphans` 走 `history::record_run(HistoryCommand::Clean, ...)`（与账本已有变体对齐），回传 `run_id` 供 `UndoToast` 一键撤销。
_理由_：撤销机制按 `run_id` + inode 身份校验确定性放回废纸篓，与命令语义无关；孤儿删除本质是"移废纸篓"，Clean 变体语义足够。若后续需按命令类型统计再引 `Orphans` 变体，属追平项非本轮必需。**执行时须核实 `HistoryCommand` 现有变体**（`crates/core/src/history.rs`），若已有更贴切变体或新增成本极低，实现者可自决——此为 KTD 而非硬约束。

**KTD5：删除端保留 `authorize_deletion` 二次校验，即便孤儿不产 Risky。**
core 保证孤儿分级只到 Moderate（`USER_DATA_SUBDIRS` 派生）+ Safe，永不 Risky（027 R3）。但 `clean_orphans` 仍调 `authorize_deletion`——纯 Safe/Moderate 批次对口令无要求（放行），含 Risky 才拒。
_理由_：纵深防御——防前端 bug 或直连 IPC 注入 Risky 项绕过；成本为零（纯 Safe/Moderate 批次无摩擦），与 clean/purge/uninstall 三处删除闸完全一致，避免安全语义在第四处漂移。

---

## Implementation Units

### U1. GUI 后端孤儿命令层

**Goal**：新增 `commands/orphans.rs`，暴露 `scan_orphans`（查询）与 `clean_orphans`（移废纸篓删）两条 Tauri 命令，加 `last_orphans` 状态槽，在 `generate_handler!` 注册。

**Requirements**：R1, R2, R4, R5, R7

**Dependencies**：无（core `scan_orphans` 已出货）

**Files**：
- `crates/gui/src/commands/orphans.rs`（新建）
- `crates/gui/src/commands/mod.rs`（加 `pub mod orphans;`）
- `crates/gui/src/lib.rs`（`AppState` 加 `last_orphans` 字段 + `Default` 初始化；`generate_handler!` 注册两命令）

**Approach**：
- `scan_orphans`：`#[tauri::command] async`，`spawn_blocking(|| Engine::scan_orphans())` → `ScanResult::from_categories(vec![CategoryGroup::new("孤儿残留".into(), items)])`（单组收纳，前端按各项 category 重分组，同 uninstall `resolve_leftovers`），存 `last_orphans`，回传 `ScanResult`。无 `begin_operation`、无 Channel（KTD1）。
- `clean_orphans`：镜像 `commands/clean::clean` 结构——`begin_operation` 取 cancel flag + `last_orphans` 句柄；短临界区 clone `select_by_paths` 出的 owned 待删项后 drop 锁（防删除全程持锁毒化槽）；`authorize_deletion`（KTD5）；`Engine::clean(refs, Trash, reporter)`；`history::record_run(HistoryCommand::Clean, ...)`（KTD4，执行时核实变体）；回传 `CleanResponse{report, run_id}`。
- 复用 `commands::{authorize_deletion, clean::select_by_paths, CleanResponse}`，不复制逻辑。

**Patterns to follow**：`crates/gui/src/commands/clean.rs`（删除结构 + 写账本 + CleanResponse）、`crates/gui/src/commands/uninstall.rs:27`（scan_uninstall 的 spawn_blocking 纯查询、无取消）、`crates/gui/src/commands/purge.rs`（last_* 独立槽取项）。

**Test scenarios**（Rust `#[cfg(test)]`，纯函数 + 授权闸层，与 clean.rs/purge.rs 测试同款）：
- Happy path：`select_by_paths` 从 orphans 槽按路径命中待删项；空选择集 → 空结果。
- 隔离（R7）：clean/purge 结果里的路径在 orphans 槽中不命中（构造两个 `ScanResult`，交叉取项断言隔离），对照同路径在自身槽正常命中以证明是隔离而非取项失效。
- 授权闸（R4/KTD5）：含 Risky 项 + 空口令 → `authorize_deletion` 拒；纯 Safe/Moderate 批次 + 空口令 → 放行（孤儿的真实分级面）。
- 未知路径被忽略：不存在的路径集 → 空结果。
- `Test expectation`：`scan_orphans`/`clean_orphans` 的 `#[tauri::command]` async 外壳依赖 Tauri runtime，不做集成测试（与既有命令一致——命令外壳无单测，逻辑下沉到纯函数与核心）；覆盖落在纯函数与 e2e（U5）。

---

### U2. 前端 IPC 绑定与类型

**Goal**：`ipc.ts` 加 `scanOrphans()` 与 `cleanOrphans()` 两个 `invoke` 包装，类型对齐后端。

**Requirements**：R1, R4, R5

**Dependencies**：U1

**Files**：
- `crates/gui/frontend/src/lib/ipc.ts`

**Approach**：
- `scanOrphans(): Promise<ScanResult>` → `invoke<ScanResult>("scan_orphans")`（无参、无 Channel，同 `scanUninstall`）。
- `cleanOrphans(paths, confirmToken, onEvent): Promise<CleanResponse>` → `invoke<CleanResponse>("clean_orphans", { paths, confirmToken, onEvent: channel })`（镜像 `clean` / `purge` 签名，含 `Channel<ProgressEvent>`）。
- 复用既有 `ScanResult`/`CleanResponse`/`ProgressEvent` 类型，无新类型。

**Patterns to follow**：`ipc.ts:210`（scanUninstall 无参查询）、`ipc.ts:196`（purge：paths + confirmToken + onEvent Channel → CleanResponse）。

**Test scenarios**：
- `Test expectation: none` —— 纯 IPC 包装（`invoke` 转发），无分支逻辑；正确性由 U5 e2e（经 tauri-mock 桩）与 TS 类型检查覆盖。

---

### U3. 「孤儿残留」路由组件

**Goal**：新建 `routes/Orphans.svelte`——单阶段相位机：进入即扫描 → 审查（全部未勾选）→ 勾选 → ConfirmDelete → 移废纸篓 → CleanReceipt + UndoToast。

**Requirements**：R1, R2, R3, R5, R6

**Dependencies**：U2

**Files**：
- `crates/gui/frontend/src/routes/Orphans.svelte`（新建）

**Approach**：
- 相位机（KTD1，较 uninstall 九态简化）：`type Phase = "loading" | "ready" | "empty" | "error" | "deleting" | "done"`；`onMount` 即 `startScan`。
- `startScan`：`setPhase("loading")` → `await scanOrphans()` → 结果空 → `empty`；非空 → 映射为 `LiveItem[]`（`selected` 取各项 preselect，天然全 false，KTD2）→ `ready`；异常 → `error`（存 message）。
- 审查态：`aggregateByCategory(items, [], true)` 重分组；`selectedItems`/`selectedSize`/`segments` 派生（同 uninstall:79-85）；每项 EvidenceCard 展示 impact/recovery 证据。
- **不预选文案（KTD2）**：列表头或空选态显式提示"孤儿残留需手动勾选要回收的项——已卸载应用的数据可能仍需保留"。
- 删除：`primaryDelete` → 有 Risky 才要求 ConfirmDelete 口令（复用 `ConfirmDelete` + `confirm.ts`）→ `cleanOrphans(paths, token, onEvent)` → `deleting`（进度经 Channel 更新 `cleaningPath`）→ `CleanResponse` → `done` + `CleanReceipt` + `UndoToast`（run_id 非空才显撤销，R5）。
- 路由级 palette 命令（U4 消费）：`ready` 态注册"重新扫描孤儿"；有选中项时注册"移入废纸篓"；`done` 态注册"重新扫描"。`registerRouteCommands(() => paletteCommands)`。
- 空/错误态各有明确 UI（R6），复用 Shell 空态/错误态样式。

**Patterns to follow**：`routes/Uninstall.svelte`（相位机 + StreamingList 动态类目 knownOrder=[] + ConfirmDelete 信任链 + CleanReceipt + UndoToast + registerRouteCommands）、`routes/Clean.svelte`（单阶段 scan→review→delete→receipt 骨架、withViewTransition 相位切换）。

**Test scenarios**：
- 主要行为覆盖落在 U5 e2e（Svelte 组件行为经 Playwright + tauri-mock 验证，与既有四路由一致——`routes/*.svelte` 无独立单测）。
- 若抽出纯函数（如相位判定 helper），随手加 vitest；否则：
- `Test expectation`：组件行为由 U5 e2e 端到端覆盖；纯派生逻辑（aggregate/format/confirm）已有 `lib/*.test.ts` 覆盖，本组件复用不重测。

---

### U4. 第五 tab 接线（App.svelte + palette 导航）

**Goal**：把「孤儿残留」接入主壳——`Tab` 联合类型、statusbar 文案、导航按钮、静态 palette 导航命令、路由渲染。

**Requirements**：R1

**Dependencies**：U3

**Files**：
- `crates/gui/frontend/src/App.svelte`

**Approach**（四个既有改点，`App.svelte` 已把每处收敛成"加一行"）：
- `import Orphans from "./routes/Orphans.svelte"`。
- `Tab` 类型加 `"orphans"`（`App.svelte:13`）。
- `TAB_LABELS` 加 `orphans: "孤儿残留模式"`（:16，避免叠三元链）。
- `staticCommands` 加 `{ id: "nav.orphans", title: "孤儿残留", keywords: ["orphans", "leftover", "guer", "canliu"], run: () => (tab = "orphans") }`（:33）。
- `<nav class="tabs">` 加导航按钮（:100，普通 accent 样式，非 explore）。
- `<main>` 渲染分支加 `{:else if tab === "orphans"}<Orphans />`（:128；注意当前 `{:else}` 落 Analyze，须改为显式 `{:else if tab === "analyze"}` + 保留 Analyze 分支，或把 orphans 插在 analyze 前）。

**Patterns to follow**：`App.svelte` 现有四 tab 接线（uninstall 分支是最贴近的加法模板）。

**Test scenarios**：
- Happy path（e2e，U5）：主界面 ready 后可见「孤儿残留」tab；点击切到孤儿路由并触发扫描。
- Palette 导航（e2e）：Cmd+K → 搜"孤儿"/"orphans" → 命中导航命令 → 切换到孤儿 tab。
- `Test expectation`：由 U5 e2e 覆盖（tab 接线是声明式，无独立单测面）。

---

### U5. 端到端测试（Playwright + tauri-mock）

**Goal**：新增 `e2e/orphans.spec.ts`，经 tauri-mock 桩验证扫描→审查→不预选→勾选→确认→删除→回执→撤销全链路 + 空/错误态。

**Requirements**：R1, R2, R4, R5, R6

**Dependencies**：U1–U4

**Files**：
- `crates/gui/frontend/e2e/orphans.spec.ts`（新建）
- `crates/gui/frontend/e2e/support/tauri-mock.ts`（扩展：加 `scan_orphans`/`clean_orphans` mock 响应）

**Approach**：镜像 `e2e/uninstall.spec.ts` / `e2e/purge.spec.ts`——在 tauri-mock 注册 `scan_orphans` 返回构造的孤儿列表（含 Safe + Moderate 项，各带 category/证据）、`clean_orphans` 返回 `CleanResponse`。断言：
- 切到孤儿 tab 后自动扫描并列出候选，附体积与证据。
- **不预选（R2）**：初始渲染无任何项被勾选，删除 CTA 无可删目标 / 提示手动勾选。
- 勾选若干项 → 删除 → 移废纸篓回执，展示释放空间；run_id 非空 → 显撤销 → 点撤销触发 `undo`。
- 空扫描（mock 返回空）→ 空态文案，不空白。
- 扫描失败（mock reject）→ 错误态，不冻结。

**Patterns to follow**：`e2e/uninstall.spec.ts`、`e2e/purge.spec.ts`、`e2e/support/tauri-mock.ts`、`e2e/support/fixtures.ts`。

**Execution note**：GUI e2e 在沙箱下 Playwright 自启 vite 会超时——手动起 dev server + `PW_NO_WEBSERVER=1` 复用（见 [[gui-e2e-sandbox-webserver-workaround]]）。CI/真机正常自启。

**Test scenarios**：本单元即测试；覆盖 R2（不预选，最高价值断言）、删除信任链、空/错误态、撤销。
- Covers R2：初始零勾选断言。
- Covers R5：删除后 run_id 驱动撤销按钮出现 + 点击触发 undo。
- Covers R6：空扫描态、扫描失败态各有断言。

---

## Scope Boundaries

### 本计划范围内
- GUI 后端 `scan_orphans`/`clean_orphans` 命令 + `last_orphans` 槽。
- 前端「孤儿残留」路由 + 第五 tab + palette 接线 + IPC 绑定。
- e2e 覆盖全链路 + 不预选 + 空/错误态。

### Deferred to Follow-Up Work
- **TUI 孤儿扫描入口**——027 同样延后 TUI 入口；TUI 从不解析残留（uninstall 亦然），孤儿是否进 TUI 属独立产品判断，本轮不做。
- **孤儿残留的规则草稿喂养**（未识别大残留一键起草成 `~/.config/mc/rules.toml`）——027 已归入 beat-mole #6 Analyze 归因那一轮。
- **`--min-age-days` 龄阈值的 GUI 可调**——core 首版硬编码 30 天（027 A2）；GUI 加滑杆/输入属追平项，待真机反馈。
- **`HistoryCommand::Orphans` 专用变体**——KTD4 复用 Clean 变体；若需按命令类型统计再引。

### 非目标（产品身份边界）
- **跨用户 / 系统级 `/Library` 孤儿**——需提权、风险面大，core 本就不扫（027 非目标），GUI 不例外。
- **LaunchAgents/LaunchDaemons 活跃 plist 反向卸载**——涉及 app 生命周期，超出纯文件残留（027 非目标）。
- **永久删除**——GUI 恒移废纸篓，无永久删除路径（全 GUI 一致的安全承诺）。

---

## System-Wide Impact

- **AppState 新增第五槽**：`last_orphans` 加入 `AppState`，`Default` 同步初始化——纯加法，不动既有四槽。
- **主壳 tab 从四增至五**：`App.svelte` 的 `Tab` 类型、TAB_LABELS、导航栏、静态 palette 命令各加一项；导航栏横向空间需目测五 tab 不挤（真机核对，必要时微调 padding）。
- **无 core 改动**：core `scan_orphans` 已出货，本计划零 core 侧改动，不影响 CLI/TUI。
- **`generate_handler!` 命令表 +2**：Tauri IPC 表加两命令，`capabilities/default.json` 若按命令白名单须同步核对（执行时核实是否需登记）。

---

## Risks & Dependencies

| 风险 | 缓解 |
|---|---|
| **不预选语义在 GUI 层被无意破坏**（如某处默认 select-all） | 核心已强制 `preselect=false`；GUI 只映射不重判（KTD2）；U5 e2e 以"初始零勾选"为最高价值断言锁死。呼应 [[forcing-one-trust-axis-misses-sibling-axis]]。 |
| **fail-closed 空结果被误当"扫描坏了"** | 空扫描态（R6）文案区分"无孤儿"与"读不到已装应用（权限）"——但 core 对后者也返回空，GUI 无法区分，故空态文案中性表述"未发现孤儿残留"，不误导。 |
| **扫描数秒无进度反馈**（同步非流式，KTD1 无进度条） | loading 态明确 spinner + 文案"正在扫描 ~/Library…"；`scan_uninstall` 端已是同款体验，用户可接受。若真机觉慢再评估是否给 core 加流式（属后续）。 |
| **五 tab 导航栏拥挤** | 真机目测；palette（Cmd+K）已是并行入口，导航栏非唯一路径。 |
| **`HistoryCommand` 变体选择**（KTD4） | 执行时核实 `history.rs` 现有变体；复用 Clean 或按需引 Orphans，实现者自决。 |

**Dependencies**：core `Engine::scan_orphans`（已出货，`engine.rs:48`）；GUI 既有命令/组件基建（clean/purge/uninstall 全套）。

---

## Verification Contract

- `cargo build` 通过；`cargo clippy --all-targets` 无新警告（pedantic 全开）。
- `cargo test -p mc-gui`（若 crate 名如此）U1 纯函数 + 授权闸测试全绿。
- `crates/gui/frontend` 下 `pnpm test`（vitest）既有测试不回归。
- `crates/gui/frontend` 下 e2e：`orphans.spec.ts` 全绿（沙箱按 [[gui-e2e-sandbox-webserver-workaround]] 手动起 dev + `PW_NO_WEBSERVER=1`）。
- 真机冒烟：GUI 启动 → 切「孤儿残留」tab → 自动扫描 → 列表全部未勾选 → 勾选一项 → 移废纸篓 → 回执 + 撤销可用。

---

## Definition of Done

- [ ] U1：`commands/orphans.rs` 两命令 + `last_orphans` 槽 + `generate_handler!` 注册；纯函数测试绿。
- [ ] U2：`ipc.ts` 两绑定，类型对齐。
- [ ] U3：`routes/Orphans.svelte` 单阶段相位机，不预选文案，删除信任链 + 回执 + 撤销。
- [ ] U4：第五 tab 接线（类型/标签/导航/palette/渲染五处），Analyze 分支不被覆盖。
- [ ] U5：`orphans.spec.ts` + tauri-mock 扩展，R2 不预选断言 + 空/错误态 + 撤销全绿。
- [ ] Verification Contract 全部通过。
- [ ] 无新 clippy 警告、无 `unsafe`、删除恒移废纸篓、授权闸保留。

---

## Sources & Research

- `docs/plans/2026-07-19-027-feat-orphan-leftover-scan-plan.md` —— 反向卸载 core + CLI 计划，Deferred 明列 GUI 入口。
- `docs/ideation/2026-07-07-next-step-tui-vs-gui.md` —— GUI 为主线的方向决策。
- 代码核实：`crates/core/src/engine.rs:48`、`crates/core/src/app_resolver.rs:365`、`crates/gui/src/commands/{clean,purge,uninstall,mod}.rs`、`crates/gui/src/lib.rs:18/71`、`crates/gui/frontend/src/App.svelte`、`crates/gui/frontend/src/routes/Uninstall.svelte`、`crates/gui/frontend/src/lib/ipc.ts`。
- Solutions：[[gui-e2e-sandbox-webserver-workaround]]、[[forcing-one-trust-axis-misses-sibling-axis]]、[[per-component-guards-miss-cross-surface-races]]。
