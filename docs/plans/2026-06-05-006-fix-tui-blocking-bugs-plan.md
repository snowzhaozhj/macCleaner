---
title: "fix: TUI 阻断级 bug 修复（第一阶段）"
status: active
origin: docs/brainstorms/2026-06-05-tui-ux-overhaul-requirements.md
date: 2026-06-05
depth: standard
---

# fix: TUI 阻断级 bug 修复（第一阶段）

## Summary

修复四个让 TUI 不可用的阻断级问题：Uninstall 同步冻结 UI、清理过程不可取消、Analyze 删除标记不执行、Sorting 取消丢失数据。

---

## Problem Frame

四个功能都存在阻断级 UX 缺陷，用户要么被卡住无法操作，要么看到死功能产生困惑。这些问题让工具无法被信任和日常使用。

(see origin: `docs/brainstorms/2026-06-05-tui-ux-overhaul-requirements.md`)

---

## Requirements

- R1: Uninstall 应用扫描不冻结 UI，有 spinner 和进度反馈
- R2: 清理过程可通过 Esc 取消，已清理部分不回滚，显示部分完成结果
- R3: Analyze 的 `d` 标记删除功能完整可用——有确认流程、能执行实际删除
- R4: Sorting 过渡状态不响应 q/Esc，不丢失扫描数据

---

## Key Technical Decisions

1. **Uninstall 复用 Clean/Purge 的后台线程模式** — 已有成熟的 `thread::spawn` + `crossbeam-channel` + Scanning 状态模式，Uninstall 对齐到同一模式而非发明新的异步方案
2. **清理取消用 cancel_flag（AtomicBool）** — 复用扫描阶段已有的取消机制，Cleaner 在每次删除操作前检查 flag
3. **Analyze 删除走 Confirming 状态复用** — 复用 Clean/Purge 已有的确认弹窗 + Cleaning 状态，而非新建 Analyze 专用删除流程
4. **Sorting 状态直接屏蔽按键** — 排序通常 < 1 秒，不值得加取消逻辑，直接不响应 q/Esc

---

## Scope Boundaries

### 不做
- 不统一键位（第二阶段）
- 不改进度信息展示（第二阶段）
- 不改扫描中提前确认（第二阶段）

### Deferred to Follow-Up Work
- 第二阶段：交互统一（P1, P3, P4, P8, P9, P11）

---

## Implementation Units

### U1. Uninstall 异步化

**Goal:** 将 `list_apps()` 从主线程同步调用改为后台线程，进入 Scanning 状态显示 spinner

**Requirements:** R1

**Dependencies:** 无

**Files:**
- `crates/tui/src/lib.rs` — Uninstall 命令入口改为 thread::spawn + channel

**Approach:**
- 参考 Clean 命令的 `handle_clean_command` 模式：`thread::spawn` 执行 `AppResolver::list_apps()` 和 `find_leftovers()`，通过 channel 发送结果
- 进入 Scanning 状态显示 spinner（"正在扫描已安装应用..."）
- 接收到结果后转入 Results 状态

**Patterns to follow:** `handle_clean_command` 和 `handle_purge_command` 中的 thread::spawn + Scanning 状态模式（`crates/tui/src/lib.rs`）

**Test scenarios:**
- Uninstall 启动后立即显示 Scanning 状态（不冻结）
- 扫描完成后正确转入 Results 显示应用列表
- 扫描过程中 Esc 可取消返回菜单
- 无已安装应用时显示空结果而非冻结

**Verification:** TUI 中选择 Uninstall 后 spinner 立即出现，不再冻结

---

### U2. 清理过程可取消

**Goal:** Cleaning 状态下 Esc 可中止清理，显示已完成部分

**Requirements:** R2

**Dependencies:** 无

**Files:**
- `crates/tui/src/lib.rs` — Cleaning 状态的按键处理
- `crates/core/src/cleaner.rs` — execute/dry_run 接受 cancel_flag 参数

**Approach:**
- `Cleaner::execute()` 接受 `&AtomicBool` 参数，每次删除操作前检查 flag
- Cleaning 状态下 Esc 设置 cancel_flag 为 true
- Cleaner 检测到取消后停止删除，返回已完成的 CleanReport（部分结果）
- 转入 Done 状态显示"已取消——已清理 X 个文件，释放 Y"

**Patterns to follow:** 扫描阶段的 `cancel_flag` 模式（`crates/tui/src/app.rs` 中的 `cancel_flag: Arc<AtomicBool>`）

**Test scenarios:**
- Cleaning 状态按 Esc 后清理停止，显示部分完成结果
- 取消前已删除的文件确实被删除（不回滚）
- 如果第一个文件删除前就取消，显示"已取消——未清理任何文件"
- 不按 Esc 时行为与当前完全一致

**Verification:** 大批量清理时按 Esc 能在 1-2 秒内停止

---

### U3. Analyze 删除标记生效

**Goal:** `d` 键标记的文件可通过确认流程执行实际删除

**Requirements:** R3

**Dependencies:** U2（复用可取消的 Cleaning 流程）

**Files:**
- `crates/tui/src/lib.rs` — Analyzing 状态新增 Enter/Delete 键处理
- `crates/tui/src/ui/analyzer.rs` — 底部提示更新

**Approach:**
- Analyzing 状态下按 Enter 或 Delete 键时，检查 `marked_for_delete` 是否非空
- 若有标记项，转入 Confirming 状态显示"确认删除 N 个项目？"
- 确认后复用 Cleaner::execute 执行删除（走 Cleaning 状态，可取消）
- 删除完成后返回 Analyzing 状态，从树中移除已删除项
- 底部提示更新：增加"Enter 删除标记项"

**Patterns to follow:** Clean/Purge 的 Confirming → Cleaning → Done 流程

**Test scenarios:**
- 无标记项时按 Enter/Delete 无反应
- 有标记项时按 Enter 弹出确认对话框，显示正确的数量和大小
- 确认后文件实际被删除（trash 或 permanent 取决于当前模式）
- 删除完成后树视图中已删除项消失
- 取消确认后返回 Analyzing，标记保留

**Verification:** TUI 中 d 标记 → Enter 确认 → 文件被删除 → 树视图更新

---

### U4. Sorting 屏蔽按键

**Goal:** Sorting 过渡状态不响应 q/Esc，防止误操作丢失扫描数据

**Requirements:** R4

**Dependencies:** 无

**Files:**
- `crates/tui/src/lib.rs` — Sorting 状态的按键处理

**Approach:**
- 移除 Sorting 状态下 q/Esc 的处理逻辑
- 排序通常 < 1 秒完成，用户几乎不会停留在此状态
- Sorting 界面已有 spinner 提示"正在排序..."

**Test scenarios:**
- Sorting 状态下按 q/Esc 无反应（不跳回菜单）
- 排序完成后正常转入 Analyzing 静态浏览状态
- 扫描数据完整保留

**Verification:** 快速连按 q 不会在排序期间丢失数据
