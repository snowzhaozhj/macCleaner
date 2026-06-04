---
title: "feat: Purge 结果页按风险分区 + 大小排序"
status: active
origin: docs/brainstorms/2026-06-04-purge-results-grouping-requirements.md
date: 2026-06-04
depth: lightweight
---

# feat: Purge 结果页按风险分区 + 大小排序

## Summary

在 TUI purge 结果页引入 SafetyLevel 分区标题行，将 10+ 个 category 按安全等级分组，组内按 total_size 降序排列。标题行不可选中，光标自动跳过。只改展示层，不动数据模型。

## Problem Frame

purge_rules 扩展到 18 条后 category 平铺列表缺乏优先级和视觉结构感，用户难以快速定位和决策。

## Requirements

- R1: category 按 SafetyLevel 分区显示（Safe → Moderate → Risky）
- R2: 每个分区内 category 按 total_size 降序排列
- R3: 分区间用彩色标题行分隔，格式 `────── {标签} ──────`
- R4: 标题行不可选中、不可展开，光标移动时自动跳过
- R5: 无结果的分区不显示标题行
- R6: 现有交互（Space/Tab/a/Enter/q）行为不变

## Key Technical Decisions

1. **在 FlatRow 枚举中新增 Separator 变体** — 比引入新的嵌套数据结构简单，对现有逻辑侵入最小
2. **排序和分区逻辑放在 `build_flat_rows()` 中** — 只改渲染时的视图组织，不影响 scan_result 数据本身
3. **光标跳过用循环实现** — Up/Down 移动后检查当前行是否为 Separator，若是则继续同方向移动

## Patterns to Follow

- 现有 `FlatRow::Category` / `FlatRow::Item` 模式（`crates/tui/src/app.rs:218-224`）
- 光标导航逻辑在 `crates/tui/src/lib.rs:389-396`（Purge Results）和 `920-927`（Uninstall Results）
- 渲染逻辑在 `crates/tui/src/ui/results.rs:79-161`

---

## Implementation Units

### U1. FlatRow 新增 Separator 变体 + build_flat_rows 排序分区

**Goal:** 让 `build_flat_rows()` 产出按风险分区、按大小排序的行列表，包含分区标题行。

**Requirements:** R1, R2, R3, R5

**Dependencies:** 无

**Files:**
- `crates/tui/src/app.rs` — 修改 `FlatRow` 枚举和 `build_flat_rows()` 方法

**Approach:**
- `FlatRow` 增加 `Separator { level: SafetyLevel }` 变体
- `build_flat_rows()` 中：先按 `dominant_safety` 分组（Safe → Moderate → Risky），组内按 `cat.total_size` 降序排序，每组开头插入一个 `Separator` 行
- 空分区（该安全等级下无 category）直接跳过不插入 Separator

**Test scenarios:**
- 只有 Safe 级别结果时，只有一个 Safe 标题行
- Safe 和 Moderate 都有结果时，先 Safe 分区再 Moderate 分区，各自标题行存在
- 同一分区内多个 category 按 total_size 从大到小排列
- 全空（无 scan_result）返回空 Vec

### U2. 渲染 Separator 行 + 光标跳过逻辑

**Goal:** TUI 正确渲染分区标题行，光标导航跳过不可选行。

**Requirements:** R3, R4, R6

**Dependencies:** U1

**Files:**
- `crates/tui/src/ui/results.rs` — 渲染 `FlatRow::Separator`
- `crates/tui/src/lib.rs` — 光标 Up/Down 后跳过 Separator 行

**Approach:**
- `results.rs` 的 `match row` 增加 `FlatRow::Separator { level }` 分支：渲染居中 `────── {label} ──────`，颜色对应安全等级，不显示 checkbox
- `lib.rs` 中 Purge Results（约 389-396 行）和 Uninstall Results（约 920-927 行）的 Up/Down 处理后，加一个 while 循环：如果新位置是 Separator 则继续同方向移动（注意边界）
- `toggle_selection` 和 `toggle_expand` 对 Separator 行 no-op（match 分支不做处理）

**Test scenarios:**
- 光标向下移动到 Separator 行时自动跳到下一个非 Separator 行
- 光标向上移动到 Separator 行时自动跳到上一个非 Separator 行
- 第一行是 Separator 时，初始光标应该在第一个 Category 行
- Space 键在 Separator 行无反应（实际不会发生因为光标跳过，但代码应安全）

## Verification

- `cargo test` 通过
- 运行 `cargo run -- purge ~` 实际看到分区标题行，Safe 组在前、Moderate 组在后，组内按大小排序
- 导航时光标不会停在标题行上
