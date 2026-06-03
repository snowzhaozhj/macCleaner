---
date: 2026-06-04
topic: analyzer-interaction-redesign
focus: 参考 dua-cli 交互设计重设计 Analyzer 扫描体验
mode: repo-grounded
---

# Ideation: Analyzer 交互重设计（参考 dua-cli）

## Grounding Context

**项目形态:** Rust workspace (core/cli/tui), ratatui + crossterm + crossbeam-channel, jwalk 并行遍历

**当前痛点:**
- Analyze 模式 Scanning→Analyzing 硬切换，30-60s 扫描期间用户不可交互
- Header 扫描路径高频闪烁（每 500 文件更新一次 progress_text）
- 完成时全屏重绘 + cursor 重置，体验断裂
- Esc 取消丢弃所有已扫描数据

**dua-cli 核心参考:**
- `scan: Option<FilesystemScan>` 叠加层模式
- `integrate_traversal_event` 逐条增量合并到 Traversal 树
- `Throttle(250ms)` AtomicBool 定时器控制 UI 刷新
- `received_events` 标志区分自动跟随 vs 用户导航
- 扫描完成 = `self.scan = None` + `recompute_sizes_recursively`
- 取消 = channel drop，已集成数据保留

## Topic Axes

1. 状态机统一 — 消除 Scanning/Analyzing 硬切换
2. 增量树构建 — entry-by-entry 逐条合并
3. 扫描中交互 — 浏览/导航已发现节点
4. 渲染节流 — 控制刷新频率、消除闪烁
5. 取消与过渡 — 优雅取消和无缝完成

## Ranked Ideas

### 1. Scan-as-Overlay 状态统一
**Description:** 删除 `AppState::Scanning` 作为 Analyze 的独占状态，改为 App 上的 `scan: Option<ActiveScan>` 字段。Analyze 启动后立即进入 `Analyzing { scanning: true }`，扫描完成时设 `scanning = false`，无状态跳变。
**Axis:** 状态机统一
**Basis:** direct: dua-cli `state.rs` `scan: Option<FilesystemScan>`
**Rationale:** 用户从第一帧就在 Analyzing 视图中，消除等待墙
**Downsides:** Analyzing 状态需在树为空/极小时渲染合理 UI
**Confidence:** 95%
**Complexity:** Medium
**Status:** Unexplored

### 2. Entry-by-Entry 增量树构建
**Description:** 后台线程通过 bounded channel 逐条发送 entry 事件，UI 线程调用 `integrate_entry` 逐条合并到 DirNode 树。用 `HashMap<PathBuf, usize>` 做 name→child_index 快查，对现有 `Vec<DirNode>` 增量插入。
**Axis:** 增量树构建
**Basis:** direct: dua-cli `integrate_traversal_event` 逐条写入 StableGraph
**Rationale:** 增量树是"扫描中导航"的基础前提
**Downsides:** 最复杂改动，中间 size 为近似值（完成后需 recompute）
**Confidence:** 90%
**Complexity:** High
**Status:** Unexplored

### 3. 全键位扫描中导航
**Description:** Analyzing 状态下无论 scanning 是否为 true，都启用完整键盘处理（Enter/Backspace/j/k/d）。crossbeam::select! 同时监听键盘和遍历事件。
**Axis:** 扫描中交互
**Basis:** direct: dua-cli `process_event` 在 scan 活跃时 select! 同等处理 terminal event
**Rationale:** 变被动等待为主动决策
**Downsides:** 未完全遍历的子目录内容动态增长
**Confidence:** 95%
**Complexity:** Medium
**Status:** Unexplored

### 4. AtomicBool Throttle 门控渲染
**Description:** 独立线程每 250ms 设 AtomicBool=true。事件集成不阻塞，但只有 `can_update()` 时才 `terminal.draw()`。键盘输入始终立即触发重绘。
**Axis:** 渲染节流
**Basis:** direct: dua-cli `Throttle` struct (common.rs L131)
**Rationale:** 渲染降为 4fps，彻底消除闪烁，可独立第一个落地
**Downsides:** 额外一个后台线程（极低成本）
**Confidence:** 95%
**Complexity:** Low
**Status:** Unexplored

### 5. 静态进度显示
**Description:** 将高频刷新路径替换为稳定统计：`已扫描 N 文件 | X GB | ~Y 文件/秒`。仅切换顶层目录时更新名称（<1Hz）。
**Axis:** 渲染节流
**Basis:** direct: dua-cli footer 只显示 entries_traversed + total_bytes
**Rationale:** 稳定数字比不可读的闪烁路径更有效
**Downsides:** 丧失细粒度路径信息（但本来就因闪烁不可读）
**Confidence:** 90%
**Complexity:** Low
**Status:** Unexplored

### 6. received_events 导航意图标志
**Description:** `received_events: bool` — 未交互时 auto-follow 最大项，用户按键后 freeze 位置。
**Axis:** 扫描中交互
**Basis:** direct: dua-cli `received_events` + `update_state_during_traversal` 条件门控
**Rationale:** 解决"看实时增长 vs 稳定浏览"冲突
**Downsides:** 极低
**Confidence:** 85%
**Complexity:** Low
**Status:** Unexplored

### 7. 取消保留部分树 + 无缝完成过渡
**Description:** (a) Esc 保留已构建树，标记 `[部分]`。(b) 完成时 recompute_sizes + 清 scanning 标记，cursor 不变。
**Axis:** 取消与过渡
**Basis:** direct: dua-cli `scan=None` + `recompute_sizes_recursively`; 取消 = channel drop + 数据保留
**Rationale:** 中途数据不应丢弃；完成不应打断浏览位置
**Downsides:** 部分树 size 是近似值需标示
**Confidence:** 90%
**Complexity:** Medium
**Status:** Unexplored

## 建议实施顺序

```
#4 Throttle → #5 静态进度 → #1 状态统一 → #2 增量树 → #3 全键位导航 → #6 received_events → #7 取消/过渡
```

前两个是独立 quick win（Low），可立即消除闪烁。后面按依赖关系串联。

## Rejection Summary

| # | Idea | Reason |
|---|------|--------|
| 1 | MVCC Arc swap 快照隔离 | 过度设计：单线程 integrate 更简单 |
| 2 | petgraph StableGraph | DirNode + Vec 足够，graph 复杂度不值得 |
| 3 | LOD 惰性子树展开 | 超出当前 scope |
| 4 | 视口驱动优先级遍历 | 高级优化，超出 scope |
| 5 | TCP 自适应节流 | 固定 250ms 已被验证足够 |
| 6 | Drop sender 取消 | 与现有 cancel_flag 冲突 |
| 7 | EntryCheck 条件验证 | 不适用（macCleaner 不做 lstat） |
| 8 | 批量 drain | 被 Throttle 方案包含 |
| 9 | 消灭 Analyzing 枚举变体 | 保留变体 + scanning: bool 更渐进 |
