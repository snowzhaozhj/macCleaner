---
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-plan-bootstrap
execution: code
type: refactor
created: 2026-07-07
---

# refactor(tui): 拆分 lib.rs 为一套内聚模块 - Plan

## Summary

`crates/tui/src/lib.rs` 现约 2465 行（生产码 ~1745 + 测试 ~668），单文件混杂主循环、鼠标、命令启动、进度事件、确认删除、分析器导航/剪树等多个关注点。本计划把生产代码按关注点拆到一组内聚模块（与已按 `AppState` 拆分的 `ui/` 对称），`lib.rs` 收敛为 ~430 行主循环骨架。**纯重构，行为不变**：验收 = `cargo test` 全绿（当前 71 项）+ `cargo clippy --workspace -- -D warnings` 零告警。

本计划取代 issue #13（原仅抽"分析器删除子系统"、降幅 8%），升级为体系拆分；并修正 issue #13 的两处假设（见 KTD1、KTD4）。

---

## Problem Frame

- **痛点**：`lib.rs` 是活跃迭代热区（近月高频改动一路加到 2465 行），单文件定位/评审成本高，多个无关关注点共处一个 `#[cfg(test)] mod tests`。
- **非目标**：不改任何运行时行为；不加功能；不做性能优化；不重写逻辑。
- **成功信号**：`lib.rs` 显著瘦身、每个关注点内聚于命名清晰的模块；测试断言集合与结果前后完全一致。

---

## Requirements

- **R1** 生产代码按关注点拆到独立模块，`lib.rs` 只保留主循环骨架（`run`/`run_app`/`handle_key`/`resolve_nav_node`/`handle_done_key` 及模块声明与顶层常量）。
- **R2** 行为零变化：`cargo test` 通过的测试集合、断言、结果与重构前逐项一致。
- **R3** `cargo clippy --workspace -- -D warnings` 零告警（pedantic 全开，不得新增 `#[allow]` 绕过）。
- **R4** 每个模块可独立成一个 commit（每步落地后 test+clippy 皆绿），便于分步验收。
- **R5** 跨模块共享符号以最小可见性暴露（`pub(crate)`），不外泄到 crate 边界之外。

---

## Key Technical Decisions

- **KTD1 — 测试留在 `lib.rs`，不随生产码搬迁（本阶段）。** 测试模块是单一 `mod tests`、共享 helper（`press`/`risky_confirm_app`/`scanning_app_with_items`/`live_app`/`results_app`）被多组测试复用。强行按模块拆测试需提升 helper 可见性或复制，风险高、收益低，且与"行为可证不变"冲突。**策略**：生产函数移到新模块并改 `pub(crate)`，测试模块仅更新引用（`use super::{X}` → `use crate::<module>::{X}`）。如此 `cargo test` 前后是同一批断言 → 行为一致的最强证明。此举修正 issue #13 "把单测从 lib.rs 拆出"的假设——纯移动阶段应最小化测试改动。测试就近搬迁列入 Deferred。

- **KTD2 — 共享符号保留在 `lib.rs` 并 `pub(crate)`。** `resolve_nav_node`（analyzer 剪树、analyzer 键盘、主键盘处理共 6+ 处用）与 `toggle_marked`（鼠标与多处键盘用）不随任一模块搬走，留在 `lib.rs` 提升为 `pub(crate)`；各模块 `use crate::resolve_nav_node` / `use crate::toggle_marked`。

- **KTD3 — 统一"自由函数 + `use crate::app::App`"，不散 `impl App`。** 众多 `fn(&mut App, …)` 本质是 `App` 操作，一律以自由函数形式随模块走，顶部 `use crate::app::{App, AppState, …}`。不把 `impl App` 方法散到多文件（避免 `App` 的实现再度碎片化）。`App`/`AppState`/`AnalyzerReturn` 已是 `pub`（`app.rs`），crate 内可直接引用。

- **KTD4 — 合并 analyzer 剪树 + analyzer 键盘为一个 `analyzer_ops` 模块。** issue #13 只抽剪树 5 函数会割裂 analyzer（键盘处理同样绕 `resolve_nav_node`）。一次性把两者收进同模块，analyzer 关注点内聚。

- **KTD5 — 模块命名与 `ui/` 对称。** 逻辑侧模块名呼应 `ui/`（`mouse`/`analyzer_ops`/`delete`/`command`/`progress`），降低导航心智负担。

---

## High-Level Technical Design

拆分后 `crates/tui/src/` 逻辑侧结构（新增 5 个模块，`lib.rs` 收敛为骨架）：

```
lib.rs  (~430)   run / run_app 主循环 · handle_key 顶层分发
                 · resolve_nav_node(pub crate) · toggle_marked(pub crate)
                 · handle_done_key · 模块声明 · 顶层常量 · #[cfg(test)] mod tests(全部留此)
 ├─ mouse.rs        滚轮 / 命中测试 / 点击定位标记
 ├─ analyzer_ops.rs 删除后剪树+导航校正(原 #13) + 分析器键盘(Analyzing/AnalyzingLive)
 ├─ delete.rs       确认接受 + 废纸篓线程 + 启动清理 (含 CONFIRM_TOKEN)
 ├─ command.rs      菜单键 + start_command(扫描/purge/analyze 线程 spawn)
 └─ progress.rs     handle_progress + analyze 编排(entry/sorting/finished/cancel/leave)
```

依赖方向单向向上：各模块 `use crate::{App, resolve_nav_node, toggle_marked}` 等，`lib.rs` 仅 `mod` 声明 + 在 `handle_key`/`run_app` 中调用各模块入口。无循环依赖。

---

## Implementation Units

单元按风险由低到高排序，每个单元 = 一次原子 commit，落地后必须 `cargo test`（71 项）全绿 + `cargo clippy --workspace -- -D warnings` 零告警方可进入下一单元。

统一手法（每个单元通用）：
1. 新建 `crates/tui/src/<module>.rs`，顶部补齐 `use`（`crate::app::…`、`mc_core::…`、`crossterm::…`、`humansize::…`、`crate::event::EventHandler` 等按需）。
2. 剪切目标函数从 `lib.rs` 到新模块；`lib.rs` 加 `mod <module>;`，并在原调用点改用 `<module>::fn` 或 `use crate::<module>::fn`。
3. 被测函数改 `pub(crate)`；测试模块 `use super::{X}` 中该符号改为 `use crate::<module>::{X}`。
4. 跨模块仍需的 `lib.rs` 私有符号（`resolve_nav_node`/`toggle_marked` 等）改 `pub(crate)`。
5. 跑 test + clippy，绿后提交。

### U1. 抽出 `mouse.rs`（第一刀，验证手法）

- **Goal**：把鼠标子系统移到 `mouse.rs`，验证"移动+改可见性+改测试 use 路径"手法在最低风险块上跑通。
- **Requirements**：R1, R2, R3, R5。
- **Dependencies**：无。
- **Files**：新增 `crates/tui/src/mouse.rs`；改 `crates/tui/src/lib.rs`。
- **搬迁函数**（原 `lib.rs` 487–696）：`toggle_marked`、`handle_mouse`、`scroll_cursor`、`mouse_scroll`、`hit_row`、`mouse_click`、常量 `MOUSE_SCROLL_STEP`。
  - **例外**：`toggle_marked` 被鼠标外的键盘处理复用 → **保留在 `lib.rs` 并 `pub(crate)`**（KTD2），`mouse.rs` 内 `use crate::toggle_marked`。其余函数随 `mouse.rs` 走。
- **可见性**：`hit_row`/`scroll_cursor` 有对应测试 → `pub(crate)`。`handle_mouse`/`mouse_scroll`/`mouse_click` 供 `lib.rs` 调用 → `pub(crate)`。
- **Approach**：`handle_mouse`/`mouse_click` 读 `AppState` 与 `Rect`，顶部 `use crate::app::App`、`use ratatui::layout::Rect`、`use crossterm::event::MouseEvent`。
- **Patterns to follow**：`tree_builder.rs`（已成功抽出的自成一体模块）为模板。
- **Test scenarios**：`Test expectation: none（纯移动）` — 无行为变更；由既有 `window_start_matches_liststate_offset_zero`、`hit_row_maps_visible_rows_and_rejects_borders`、`hit_row_accounts_for_scroll_offset`、`hit_row_click_below_last_item_is_none`、`scroll_cursor_steps_and_clamps` 守护（更新其 `use` 路径后须原样通过）。
- **Verification**：`cargo test` 71 项全绿；`cargo clippy --workspace -- -D warnings` 零告警；`lib.rs` 减约 200 行。

### U2. 抽出 `analyzer_ops.rs`（合并剪树 + 分析器键盘，取代 issue #13）

- **Goal**：把分析器删除后剪树/导航校正与分析器键盘处理一并收进 `analyzer_ops.rs`，一次内聚 analyzer 关注点。
- **Requirements**：R1, R2, R3, R5；对应 issue #13。
- **Dependencies**：U1（复用同一手法；无代码耦合）。
- **Files**：新增 `crates/tui/src/analyzer_ops.rs`；改 `crates/tui/src/lib.rs`。
- **搬迁函数**（原 `lib.rs` 1432–1795）：`collect_marked`、`restore_analyzer_after_delete`、`prune_paths`、`nav_path_target_paths`、`clamp_nav_after_prune`、`handle_analyzer_key`、`handle_analyzer_live_key`。
  - **保留在 `lib.rs`**：`resolve_nav_node`（1427，多处共享）→ `pub(crate)`；`analyzer_ops.rs` 内 `use crate::resolve_nav_node`。
- **可见性**：`collect_marked`/`restore_analyzer_after_delete`/`prune_paths`/`clamp_nav_after_prune`/`nav_path_target_paths`（后者被测试 `super::` 引用）→ `pub(crate)`。键盘入口 `handle_analyzer_key`/`handle_analyzer_live_key` → `pub(crate)`。
- **Approach**：`restore_analyzer_after_delete` 触碰 `App`/`AppState`/`AnalyzerReturn`，顶部 `use crate::app::{self, App, AppState}`、`use mc_core::models::DirNode`、`use humansize::{format_size, DECIMAL}`。键盘函数可能调 `crate::delete`（若 U3 已落）或 `start_cleaning_from_analyzer`——本单元先保持对 `lib.rs`/后续模块入口的调用路径，函数移动不改调用语义。
- **Patterns to follow**：`resolve_nav_node` 的共享符号处理同 U1 的 `toggle_marked`。
- **Test scenarios**：`Test expectation: none（纯移动）` — 由 `collect_marked_prunes_marked_dir_and_recurses_unmarked`、`restore_analyzer_prunes_only_succeeded_and_keeps_failed`、`prune_paths_removes_marked_child_and_recomputes_size`、`prune_paths_removes_nested_file_and_rolls_up_size`、`clamp_nav_truncates_when_target_dir_deleted_and_clamps_cursor`、`clamp_nav_follows_target_by_path_after_earlier_sibling_removed`、`live_*` 系列守护（更新 `use`/`super::` 路径后原样通过）。
- **Verification**：同 U1；`lib.rs` 再减约 360 行。

### U3. 抽出 `delete.rs`

- **Goal**：删除执行子系统（确认接受、废纸篓线程、启动清理）内聚。
- **Requirements**：R1, R2, R3, R5。
- **Dependencies**：U2（`handle_analyzer_*` 调 `start_cleaning_from_analyzer`；移动后改 `use crate::delete::start_cleaning_from_analyzer`）。
- **Files**：新增 `crates/tui/src/delete.rs`；改 `crates/tui/src/lib.rs`（及 U2 的 `analyzer_ops.rs` 调用点）。
- **搬迁函数**（原 `lib.rs` 1299–1416）：常量 `CONFIRM_TOKEN`、`confirm_accept`、`spawn_trash_thread`、`start_cleaning`、`start_cleaning_from_analyzer`。
- **可见性**：`CONFIRM_TOKEN` 现为 `pub const`（对外 API，保持 `pub`，从 `crate::delete` 重导出或在 `lib.rs` `pub use crate::delete::CONFIRM_TOKEN` 以不破坏现有 `pub` 路径）。函数供 `lib.rs`/`analyzer_ops` 调用 → `pub(crate)`。
- **Approach**：`spawn_trash_thread` 走 `EventHandler` + `thread::spawn`，顶部 `use crate::event::EventHandler`、`use crate::app::App`。
- **Test scenarios**：`Test expectation: none（纯移动）` — 由 `risky_confirm_enter_does_not_delete`、`risky_confirm_wrong_token_does_not_delete_and_esc_cancels`、`confirm_token_is_ascii_lowercase` 守护。**注意** `CONFIRM_TOKEN` 的 `pub` 可见性路径不得改变（安全语义，`ce-brainstorm`/CONCEPTS 强调）。
- **Verification**：同上；`lib.rs` 再减约 115 行。

### U4. 抽出 `command.rs`

- **Goal**：命令启动（菜单键 + 扫描/purge/analyze 线程装配）内聚。
- **Requirements**：R1, R2, R3, R5。
- **Dependencies**：U1–U3（`start_command` 与各命令入口稳定后再移）。
- **Files**：新增 `crates/tui/src/command.rs`；改 `crates/tui/src/lib.rs`。
- **搬迁函数**（原 `lib.rs` 697–888）：`handle_menu_key`、`start_command`。
- **可见性**：`pub(crate)`；`lib.rs` 的 `handle_key`/`handle_menu_key` 调用点改 `command::…`。
- **Approach**：`start_command` spawn 后台线程并装配 `reporter::TuiReporter`，顶部 `use crate::{reporter, event::EventHandler, app::App}`、`use mc_core::engine::Engine` 等按现状。
- **Test scenarios**：`Test expectation: none（纯移动）` — 无直接单测覆盖该块（现状即无）；靠编译 + 全量 `cargo test` 回归守护。
- **Verification**：同上；`lib.rs` 再减约 190 行。

### U5. 抽出 `progress.rs`（最热区，最后做）

- **Goal**：进度事件 + analyze 编排内聚，完成骨架收敛。
- **Requirements**：R1, R2, R3, R5。
- **Dependencies**：U1–U4。
- **Files**：新增 `crates/tui/src/progress.rs`；改 `crates/tui/src/lib.rs`。
- **搬迁函数**（原 `lib.rs` 889–1219）：`handle_progress`、`handle_analyze_entry`、`transition_to_sorting`、`handle_analyze_finished`、`cancel_analyze_to_menu`、`request_leave_to_menu`。
- **可见性**：`pub(crate)`；`request_leave_to_menu` 被测试 `super::` 引用 → 测试改 `use crate::progress::request_leave_to_menu`。
- **Approach**：`handle_progress` 与 `reporter.rs` 强相关，顶部 `use crate::app::{App, AppState}`、`use mc_core::progress::ProgressEvent`。此块最大最热——一次性整体移动、不拆逻辑，降低与后续开发撞车面。
- **Test scenarios**：`Test expectation: none（纯移动）` — 由 `found_merges_repeated_category_path_and_counts_distinct_items`、`found_seeds_preselect_by_safety_and_flag`、`found_ignored_outside_scanning_state`、`scanning_*` 系列、`request_leave_to_menu_two_step_confirm_when_marked`、`request_leave_to_menu_immediate_when_no_marks` 守护。
- **Verification**：同上；`lib.rs` 收敛至 ~430 行骨架。

---

## Scope Boundaries

**In scope**：上述 5 个模块的纯移动拆分 + 共享符号 `pub(crate)` 化 + 测试 `use` 路径更新。

### Deferred to Follow-Up Work
- **测试就近搬迁**：把各模块对应测试从 `lib.rs` 的 `mod tests` 迁到各模块的 `#[cfg(test)] mod tests`（需处理共享 helper 提升/复制）。本阶段为保"行为可证不变"刻意不做（KTD1）。
- **`progress.rs` 进一步细分**：若日后 analyze 编排与 scan 进度继续膨胀，可再分 `progress/` 子模块。
- **`impl App` 归整**：本计划坚持自由函数（KTD3），不在此阶段调整 `App` 方法布局。

**Out of scope**：任何运行时行为、功能、性能、安全模型改动。

---

## Risks & Mitigation

- **R-a 移动遗漏私有依赖致编译失败** → 逐单元编译；`cargo clippy` 的 `dead_code`/未解析符号即时暴露；每单元独立 commit 便于二分。
- **R-b 测试 `use`/`super::` 路径改错致测试静默失效** → 每单元后核对 `cargo test` **通过项数不减**（应恒为 71）；数目下降即有测试被移除/未编译。
- **R-c `CONFIRM_TOKEN` 可见性回退破坏安全语义** → U3 显式保留 `pub` 对外路径（`pub use` 重导出），并有 `confirm_token_is_ascii_lowercase` 等守护。
- **R-d 与主线并发开发冲突（progress 最热）** → 排最后（U5）；worktree 隔离；一次整体移动不拆逻辑。

---

## Verification Contract

每个单元落地后、以及最终合并前均须满足：
1. `cargo test`：**71 项全绿**，通过项数与重构前逐项一致（R2）。
2. `cargo clippy --workspace --all-targets -- -D warnings`：零告警，无新增 `#[allow]`（R3）。
3. `cargo build`：workspace 编译通过。
4. 结构核验：`wc -l crates/tui/src/lib.rs` 收敛至 ~430；新增 5 个模块文件各自内聚。
5. 行为一致性论证：因测试断言集合未变（KTD1），全绿即等价于行为不变的直接证据；无需额外运行时对拍。

## Definition of Done

- R1–R5 全部满足；`lib.rs` 收敛为主循环骨架 + 5 个内聚模块。
- Verification Contract 五项全过。
- 每个单元为一个清晰 commit；PR 描述说明"纯重构、行为不变、测试断言集合未变"。
- 更新/关闭 issue #13（本计划取代之，含合并 analyzer + 修正测试拆分假设的说明）。
