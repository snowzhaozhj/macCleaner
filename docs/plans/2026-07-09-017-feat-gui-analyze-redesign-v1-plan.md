---
title: macCleaner GUI Analyze 重设计 v1（普通用户优先·止跳变·空间地理分区） - Plan
type: feat
date: 2026-07-09
topic: gui-analyze-redesign-v1
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-brainstorm
execution: code
---

# macCleaner GUI Analyze 重设计 v1 - Plan

## Goal Capsule

- **Objective:** 把 GUI 的 **Analyze（分析）** 标签页从"旧 TUI 移植形态"提升到 Clean 已达的 v1 品质——稳定不跳变的外壳 + 首屏一句话答案 + move 5「安全空间地理分区」（macOS 储存空间式分段横条），并让删除的信任回路（诚实 Trash 提示）与 Clean 对齐。
- **Product authority:** 用户（产品负责人）＋ `STRATEGY.md`（三层界面共享同一引擎、GUI 面向普通用户）＋ `docs/plans/2026-07-08-015-feat-gui-redesign-v1-plan.md`（v1 重设计基线，move 5 已在 Scope Boundaries 立项）。
- **Open blockers:** 无。范围内为纯呈现层改动，不触 `mc-core` 引擎与后端命令契约。
- **执行画像:** 只改 `crates/gui/frontend/src/routes/Analyze.svelte` 及少量 `lib/` 组件；复用 v1 已出货的 `SummaryHeader`（需文案适配）、`UndoToast`、`ConfirmDelete`、`format.ts` 原语。**在 branch/worktree 上执行**（main 禁止改源码）。

> **来源核实（Phase 1.1 grounding）：** v1 重设计（015）仅覆盖 Clean（`Clean.svelte` 已用 `SummaryHeader`/`StreamingList`/`CleanReceipt`/`UndoToast`）；`Analyze.svelte`（520 行）仍是独立内联渲染，相位切换整块 mount/unmount、无首屏答案、无 Trash 提示 toast、等宽文本网格。删除的安全语义（`classifyMarked` 回查分级 → `ConfirmDelete` type-to-confirm → `deleteMarked` 默认 Trash → 原地 `pruneTree`）已健壮，**本次不动**。后端 `analyze` 命令当前返回**完整树 + Progress 累加**（非增量树事件），故"扫描期逐项树流式"不在本版范围。

---

## Product Contract

### Summary

把 Analyze 重设计为：稳定不跳变的外壳，打开分析即一句"主目录占用 X GB"＋ macOS 储存空间式分段横条（当前层 top 消费者、低饱和分类色、图例带精确值），逐层导航时横条按当前层重构；扫描期只增长总量、不新增/移除行，结果就绪时一次 FLIP settle（体积降序）。删除完整继承现有安全语义，并补齐与 Clean 一致的诚实"已移到废纸篓·在访达中恢复"提示。纯呈现层复用 `mc-core`，暗色优先。

### Problem Frame

GUI 的战略目的是触达 CLI/TUI 到不了的**普通 Mac 用户**，而普通用户对 GUI 的核心诉求正是"看清什么占空间"＝ Analyze。但 v1 重设计只把 Clean 提到位，Analyze 仍停在旧 TUI 移植：① 相位切换（idle→analyzing→ready）整块 DOM 增删，扫描完成瞬间列表凭空出现——跳变摧毁信任；② 无首屏答案，直接抛出等宽文本行网格，普通用户看不到"总量/分布"的空间感；③ 终端残留美学（全等宽、固定 34ch 名列、无字阶层次）；④ 删除后只有一行 statusbar，缺少 Clean 那句诚实的"已移到废纸篓·可在访达恢复"，信任回路不完整。MVP 唯一未达 v1 的面，就在这里。

### Key Decisions

- **只改呈现层，完整继承安全语义。** 复用 `mc-core` 的 `analyze`/`classify_marked`/`delete_marked` 命令与 `ProgressReporter`；不改扫描/删除引擎、不改后端命令契约、不改 CLI/TUI。删除仍走 `classifyMarked` 回查 → `ConfirmDelete`（Risky 项 type-to-confirm）→ 默认 Trash → 原地 `pruneTree`。
- **move 5「安全空间地理分区」以分段横条实现，不做矩形 treemap。** 首屏与每个导航层用 macOS 储存空间式的**单条分段横条**（当前层 top-N 子项占比 + 低饱和分类色 + 图例精确值）给出空间感；矩形 treemap 复杂度高、收益边际，留作后续（YAGNI）。
- **扫描期不流式逐项树（受后端契约约束）。** 后端 `analyze` 返回完整树 + Progress 累加，故扫描期呈现"稳定外壳 + 增长的总量 + 骨架"，就绪时一次性 settle。真正的增量树流式需后端改造，独立立项。
- **复用 v1 原语，Clean 与 Analyze 收敛到同一套呈现语言。** `SummaryHeader` 适配"占用"文案（Clean 为"可安全释放"）；删除提示复用 `UndoToast`（"已移到废纸篓·在访达中恢复"，`open_trash` 已有）。impeccable 5 原则 = 硬约束（同 015 R17–R19：字阶 ≤3 级、tabular-nums、动效只传达状态）。

### Actors

- **A1. 普通 Mac 用户（首要）** — 磁盘告警时想"看清什么占空间并安全清出"，需要首屏就有总量与空间分布，不需要读等宽路径网格。
- **A2. Mac 开发者（次要）** — 靠逐层导航 + 逐项体积做精确审查；导航/标记/删除能力保持不退化。

### Key Flows

- **F1. 看清占用（默认路径）。** 打开分析（FDA 已授权）→ 扫描期稳定外壳 + 增长总量 + 骨架 → 就绪：首屏"主目录占用 X GB"＋分段横条＋图例，下方逐层列表按体积降序（一次 settle）。
- **F2. 逐层钻取。** 点子目录进入下一层 → 分段横条按当前层 top 消费者重构，面包屑可回溯；标记大项。
- **F3. 安全清出。** 勾选 → "删除标记" → `classifyMarked` 回查分级 → `ConfirmDelete`（含 Risky 则 type-to-confirm）→ 移废纸篓 → 单实例 toast"已移到废纸篓·在访达中恢复" → 原地剪树，计数与列表即时更新。

### Requirements

**稳定基座与防跳变**

- R1. 外壳三区（摘要 / 列表 / 操作）稳定：相位切换（idle→analyzing→ready→deleting）只替换槽位内容，摘要区与列表区的 DOM 容器永不 mount/unmount。
- R2. 扫描期行序锁死、不新增/移除行；就绪时用 resolved 树做一次 FLIP settle（体积降序，≤250ms）。导航切层时对新层做一次 settle。
- R3. 扫描期呈现骨架 + 增长的总量数字（tabular-nums，不抖动）；不做 count-up 补间。

**首屏答案与空间地理分区（move 2 + move 5）**

- R4. 就绪首屏呈现"主目录占用 X GB"＋ macOS 储存空间式静态分段横条（当前层 top-N 子项，低饱和分类色，图例带精确值）＋下方逐层列表。
- R5. 分段横条随导航层级重构：进入子目录后，横条表达**当前层**的空间分割；根层表达主目录分割。
- R6. 主数字来自当前层/扫描累加，无需新数据源；扫描期显示实时累加总量，就绪后定格为该层总量。

**终端残留物移除与设计一致性**

- R7. 移除等宽一辺倒网格：应用字阶层次（≤3 级）、tabular-nums 数字列、去掉固定 34ch 名列的僵硬布局；符合 impeccable 5 原则（同 015 R17–R19 基准）。
- R8. 逐层列表行保留每项体积、相对占比条、进入/标记手势，但视觉与 Clean 收敛（暗色优先、低饱和）。

**删除与信任回路（完整继承 + 补齐提示）**

- R9. 删除完整继承现有安全语义（不退化）：`classifyMarked` 回查安全分级 → `ConfirmDelete`（任一 Risky 项触发 type-to-confirm，输入 `delete`，Enter 不代替确认）→ `deleteMarked` 默认移废纸篓 → 原地 `pruneTree` 并清理失效的后代标记。
- R10. 删除完成后呈现与 Clean 一致的单实例诚实提示"已移到废纸篓 · 在访达中恢复"（复用 `UndoToast` + `open_trash`）；文案表述为"已移到废纸篓，可恢复"，不表述为"已删除"，不提供永久删除路径。
- R11. 无静默删除：执行前每个将删项及其大小、安全等级在 `ConfirmDelete` 可见可审；权限不足被跳过的路径不静默省略（沿用现有 FDA 引导，Analyze 不新增权限逻辑）。

### Success Criteria

- 扫描→结果→删除的相位切换中，摘要区与列表容器不 mount/unmount（录屏/DOM 快照可验）。
- 就绪首屏出现"主目录占用 X GB"＋分段横条＋图例；进入子目录后横条按当前层重构。
- 扫描期无行新增/移除，就绪时恰好一次体积降序 settle。
- 删除：Risky 项强制 type-to-confirm；成功后出现"已移到废纸篓·在访达中恢复"toast；剪树后计数/列表/横条即时一致。
- 全 workspace `cargo clippy --all-targets`(pedantic) + `cargo test` 通过；前端 `pnpm test`（vitest）+ 类型检查通过；`e2e/analyze.spec.ts` 通过（必要时随呈现变化更新断言，不放宽安全断言）。

### Scope Boundaries

**In scope**
- 重写/重构 `crates/gui/frontend/src/routes/Analyze.svelte`（呈现层）。
- `SummaryHeader` 文案适配（"占用" vs "可安全释放"，最小改动，不破坏 Clean 用法）或新增薄封装。
- 复用 `UndoToast`/`ConfirmDelete`/`format.ts`；必要时抽出层级分段计算到 `lib/`（带单测）。
- 更新/新增前端单测与 `e2e/analyze.spec.ts` 断言。

**Out of scope（后续，不在本版）**
- 后端 `analyze` 增量树事件化 / 扫描期逐项树流式（需改后端契约）。
- 矩形 treemap 可视化（本版以分段横条满足 move 5）。
- move 6 渐进披露（展开=换问题+等价CLI）、move 7 purge/uninstall GUI 入口 + 顶部导航 + Cmd+K、真一键 undo（`mc undo`）、仪表盘。
- 任何 `mc-core` 引擎 / 后端命令契约 / CLI / TUI 改动。

### Outstanding Questions（已在实现中定夺）

- **SummaryHeader 复用方式** → 把 prop `selectedSize` 改名为通用 `amount` 并新增 `lead` prop（默认"可安全释放"）；Clean 传 `lead` 缺省，Analyze 传 `lead="占用"`、`amount`=当前层总占用。两路由共用同一首屏组件，无重复实现。
- **分段横条 top-N** → top 5 + 其余合并为单个"其他"段（新纯函数 `dirSegments`，带单测）。逐层导航时以当前层子项重算。
- **扫描期呈现** → 摘要区 `SummaryHeader` 常驻（`amount`=实时累加总量 + "扫描中…"标签 + 空横条），列表区渲染 6 行等高骨架；就绪时内容替换为分段横条 + 逐层列表，DOM 容器不 mount/unmount。

### Implementation Summary（本 PR 实际改动）

- `crates/gui/frontend/src/routes/Analyze.svelte` — 用 `Shell` 三区快照重构（稳定基座），接入 `SummaryHeader`（占用 + move 5 分段横条）、`UndoToast`（诚实 Trash 提示）；名列改 UI 字体 + 弹性宽度、数字 tabular-nums（去终端等宽网格）；扫描期骨架行；删除安全语义（`classifyMarked`→`ConfirmDelete`/type-to-confirm→`deleteMarked`=Trash→`pruneTree`）完整保留。
- `crates/gui/frontend/src/lib/format.ts` — 新增纯函数 `dirSegments`（move 5）。
- `crates/gui/frontend/src/lib/SummaryHeader.svelte` — `selectedSize`→`amount` + 新增 `lead` prop（通用化，Clean/Analyze 共用）。
- `crates/gui/frontend/src/routes/Clean.svelte` — 跟进 `SummaryHeader` 新 prop 名。
- `crates/gui/frontend/src/lib/format.test.ts` — 新增 `dirSegments` 单测（降序 / top-N 合并 / 边界 / 不除零）。

### Verification（全绿）

- `pnpm check`（svelte-check）0 error；`pnpm build` 通过；`pnpm test`（vitest）43 passed；`pnpm e2e` 16 passed（含 Clean 撤销吐司回归、Analyze 主干 + Risky type-to-confirm）。
- `cargo clippy --all-targets`（pedantic）无警告；`cargo test` 全通过（未改 Rust，回归确认）。

