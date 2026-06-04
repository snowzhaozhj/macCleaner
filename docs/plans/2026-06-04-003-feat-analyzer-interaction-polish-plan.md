---
title: "feat: Analyzer 扫描中交互体验完善"
type: feat
status: active
date: 2026-06-04
origin: docs/ideation/2026-06-04-analyzer-interaction-redesign.md
---

# Analyzer 扫描中交互体验完善

## Summary

完成 Analyzer 交互重设计的最后两项：(1) `received_events` 导航意图标志，让扫描中列表自动跟随最大子项，用户按键后冻结位置；(2) 取消扫描时保留已构建的部分树供浏览，而非丢弃回到菜单。两项改动均 Low-Medium 复杂度，共约 60-80 行 diff。

## Problem Frame

当前 Analyzer 扫描中体验有两个缺陷：扫描过程中列表顺序随 size 变化而跳动，用户难以追踪感兴趣的目录（因为排序在 finalize 后才生效，live 模式下顺序按发现时间而非大小）；按 Esc 取消扫描会丢弃全部已扫描数据直接回到菜单，用户在长时间扫描后被迫重新开始。

## Requirements

- R1. 扫描中 `AnalyzingLive` 状态下，若用户未进行过任何导航操作（上下移动、进入子目录），cursor 应自动跟随当前层级的第一项（保持 cursor=0），列表按发现顺序自然增长
- R2. 用户在 `AnalyzingLive` 中按下任何导航键（j/k/Up/Down/Enter/Backspace）后，cursor 位置冻结，不再受新 entry 集成的影响
- R3. 从 `AnalyzingLive` 中按 Esc/q 取消扫描时，已构建的部分树应保留并转入 `Analyzing` 状态供浏览
- R4. 部分树的 UI 应有明确标识（标题显示 `[部分]`），让用户知道数据不完整
- R5. 部分树在转入 `Analyzing` 前应执行 finalize 排序（通过现有的 `Sorting` 过渡态）

## Key Technical Decisions

- **`received_events` 作为 `AnalyzingLive` 状态字段**：在 `AppState::AnalyzingLive` 中新增 `user_navigated: bool` 字段，而非作为 `App` 顶层字段。因为该标志的生命周期与 `AnalyzingLive` 状态完全一致，不需要跨状态保留。命名采用 `user_navigated` 比 dua-cli 的 `received_events` 更语义化。
- **取消复用现有 Sorting 过渡态**：取消扫描后不直接将 owned `DirNode` 包装为 `Arc<DirNode>` 进入 `Analyzing`，而是经过 `Sorting` 状态执行 finalize 排序。这复用了现有的后台排序 + channel + 错误恢复代码路径，无需新增状态或 channel。
- **部分标识通过 `Analyzing` 新增 `partial: bool` 字段**：让渲染层根据该字段决定是否显示 `[部分]` 标签。比在标题字符串中 hardcode 更干净，也方便后续扩展（如显示扫描覆盖率百分比）。

## Implementation Units

### U1. 添加 `user_navigated` 导航意图标志

- **Goal:** 在 AnalyzingLive 状态中跟踪用户是否已进行导航操作
- **Requirements:** R1, R2
- **Dependencies:** 无
- **Files:**
  - `crates/tui/src/app.rs` — `AnalyzingLive` 变体新增 `user_navigated: bool` 字段
  - `crates/tui/src/lib.rs` — 初始化 `user_navigated: false`；`handle_analyzer_live_key` 中导航键设 `user_navigated = true`；`handle_analyze_entry` 中当 `user_navigated == false` 时保持 `cursor = 0`
- **Approach:** 在 `handle_analyzer_live_key` 中，对 `Up/Down/j/k/Enter/Backspace` 分支设置 `*user_navigated = true`。在 `handle_analyze_entry` 中（处理 `AnalyzeEvent::Entry` 时），检查 `user_navigated`：若为 false 则不修改 cursor（当前实现已不修改 cursor，需确认）。关键是确保新 entry 插入不会意外移动未导航用户的视口。
- **Patterns to follow:** 现有 `AnalyzingLive` 字段初始化模式（`lib.rs` `start_command` 中 `ActiveCommand::Analyze` 分支）
- **Test scenarios:**
  - 启动 Analyze 后不按任何键，cursor 保持在第一项（index 0），新目录出现不影响位置
  - 按 j 向下移动后，后续新 entry 不改变 cursor 位置
  - 按 Enter 进入子目录后 `user_navigated` 保持 true
  - 按 Esc 回到根层后 `user_navigated` 保持 true（不重置）
- **Verification:** 启动 Analyze 扫描 `~`，观察列表增长时 cursor 不跳动；按 j 移动后 cursor 冻结

### U2. 取消扫描保留部分树

- **Goal:** Esc/q 取消扫描时保留已构建的树，经 Sorting 排序后进入 Analyzing 供浏览
- **Requirements:** R3, R4, R5
- **Dependencies:** U1（`user_navigated` 字段需已存在，因为 `AnalyzingLive` 结构变更应在同一轮完成）
- **Files:**
  - `crates/tui/src/app.rs` — `Analyzing` 变体新增 `partial: bool` 字段
  - `crates/tui/src/lib.rs` — 修改 `abort_analyze` 函数：不再直接回 Menu，而是取出 tree_root 进入 `Sorting`；Sorting 完成后构建 `Analyzing { partial: true, ... }`；新增从 `Sorting` 回到 `Analyzing` 时传递 `partial` 标志的逻辑
  - `crates/tui/src/ui/analyzer.rs` — `draw` 函数根据 `partial` 显示 `[部分]` 标签
- **Approach:** 
  - `abort_analyze` 当前逻辑：设 `Menu` + 清理 `analyze_rx`/`tree_builder`/`active_command`。改为：取出 `AnalyzingLive` 的 `tree_root`/`marked_for_delete`/`cursor_stack`，清理 `analyze_rx`/`tree_builder`，进入 `Sorting` 状态，与 `handle_analyze_finished` 相同的后台排序流程。
  - 需要区分"正常完成的 Sorting"和"取消后的 Sorting"。方案：在 `Sorting` 状态中新增 `partial: bool` 字段，Sorting 完成后传递给 `Analyzing`。
  - `handle_analyze_finished` 设 `Sorting { partial: false, ... }`；`abort_analyze` 设 `Sorting { partial: true, ... }`。
  - 渲染：`draw` 中检查 `partial`，在标题或统计栏显示 `[部分扫描]`。
- **Patterns to follow:** `handle_analyze_finished`（`lib.rs`）中的 `Sorting` 过渡逻辑；`draw_sorting`（`ui/analyzer.rs`）
- **Test scenarios:**
  - 扫描进行中按 q，应看到 Sorting spinner 然后进入排序后的树浏览（非 Menu）
  - 部分树应显示 `[部分扫描]` 标识
  - 部分树中的目录应按 size 正确排序
  - 部分树中可以正常导航（Enter/Backspace/j/k/d 标记）
  - 在部分树浏览中按 q 应回到 Menu
  - 扫描进行中按 Esc（非根层），应正常返回上级目录（不触发取消）
  - 扫描进行中在根层按 Esc，应触发取消 → Sorting → 部分树浏览
- **Verification:** 启动 Analyze，等 3-5 秒后按 q，确认看到排序过渡后进入带 `[部分扫描]` 标识的树浏览界面；确认目录排序正确；确认 q 再次退出回到 Menu

## Scope Boundaries

### Deferred to Follow-Up Work

- 部分树的扫描覆盖率百分比显示（需要预估总文件数）
- "继续扫描"功能（从部分树恢复扫描）
- cursor auto-follow 最大项（当前 R1 只保持 cursor=0，auto-follow 需要每次 integrate_entry 后重新排序当前层级，性能开销较大，暂不实施）

## Sources and Research

- dua-cli `received_events` 标志实现：`src/interactive/app/state.rs` — `received_events: bool` 字段
- dua-cli 取消保留数据：channel drop 后已集成数据保留在 `Traversal` 中
- macCleaner 现有 `abort_analyze` 函数：`crates/tui/src/lib.rs`，当前行为是清理所有状态回到 Menu
- macCleaner `Sorting` 过渡态：`crates/tui/src/lib.rs`，已有完整的后台排序 + channel + panic 恢复
