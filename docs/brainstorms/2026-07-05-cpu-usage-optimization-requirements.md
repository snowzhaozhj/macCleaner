---
title: 扫描期 CPU 占用优化（测量驱动）
date: 2026-07-05
status: requirements
type: performance
---

# 扫描期 CPU 占用优化（测量驱动）

## 问题

交互式使用 `mc`（TUI）时，扫描过程中活动监视器观察到进程 CPU **稳定在 150-200%**，几个命令（Analyze / Clean / Purge / Uninstall）都差不多，直到扫描完成才降下来。对一个"清理工具本身应保持轻量、不成为新的系统负担"（见 `STRATEGY.md`）的产品，这与定位不符。

同时，**扫描速度是硬约束**——`STRATEGY.md` 的关键指标是"全盘扫描 < 30s"。任何降 CPU 的手段都不能让扫描变慢。

## 已核实的归因（本次调研）

CPU 行为干净地分成两态：

- **静止态**（Menu / Results / Analyzing 浏览 / Done）：主循环纯阻塞 `select`（`crates/tui/src/lib.rs:233`），近乎零 CPU。**现状良好，本次不得回退。**
- **扫描/分析态**：CPU 全部在此。

150-200% 超过 1.5-2 个核，**数学上不可能主要来自渲染**——TUI 渲染在单一主线程（最多吃满一个核 = 100%），且重绘已被节流到 ~10fps（`crates/tui/src/throttle.rs`，80ms）、单帧仅毫秒级。因此 150-200% 的**主体只能是多线程后台扫描池**：

- Clean：3 个 jwalk 遍历线程（`crates/core/src/scanner.rs:28` `WALK_THREADS`）
- Purge：3 个遍历线程 + 4 个目录大小计算线程（`crates/core/src/scanner.rs:524` `build_dir_size_pool`，并行 stat 海量条目）
- Analyze：遍历线程 + **每个文件系统条目一次 channel 发送**（`crates/tui/src/lib.rs:574`）

渲染层浪费真实存在但属**次要**（顶多一个核的一部分）：

| 热点 | 位置 | 代价 |
|---|---|---|
| 每帧全量排序当前目录 | `crates/tui/src/ui/analyzer.rs:94` `size_desc_order` | AnalyzingLive 每帧 O(n log n)，结果算完即弃、无缓存 |
| 每帧重建扁平列表 + 全部 ListItem（不做视窗裁剪） | `crates/tui/src/ui/rows.rs:25,52` | Scanning 时开销随发现项数无上限增长 |
| 无状态变化也每 ~100ms 重绘 | `crates/tui/src/lib.rs:222` | 纯为转 spinner，卡在大目录时也空转 |
| 一事件一 select 迭代，无批量 drain | `crates/tui/src/lib.rs:126` | 百万级事件逐个过循环 |

**核心洞察**：文件系统遍历大多是 **I/O 密集**而非 CPU 密集。若线程主要在等磁盘，多线程空转是在浪费 CPU 却没换来等比速度——**适当调低并发，可能 CPU 降一大截而墙钟时间几乎不变**。但这是否成立取决于磁盘/缓存状态（SSD 热缓存下可能反而是 CPU 密集，减线程即变慢）——**必须实测，不能拍脑袋**。这正是本需求采用"测量驱动"的原因。

## 目标 / 非目标

**目标**
- 扫描期进程 CPU 从 150-200% 显著下降。
- 扫描墙钟时间相对基线不退化（对齐"全盘 < 30s"）。
- 不改任何清理规则、安全分级（`SafetyLevel`）、删除语义。

**非目标**
- 不改 CLI 的 indicatif 进度路径（高 CPU 是交互态 TUI，非 CLI）。
- 不为降 CPU 牺牲扫描速度——速度是硬约束。
- 不做 GUI 层（尚未存在）。
- 不重构静止态（现已近零 CPU）。

## 范围

### 阶段一：测量归因（前置，必做）

产出决策所需的数据，避免在猜测上做并发权衡：

- **按线程剖析扫描期 CPU**：遍历线程 vs 目录大小线程 vs 主线程事件处理各占 150-200% 的多少（macOS 可用 `sample` / Instruments Time Profiler 按线程，或 `ps -M`；线程已命名，如 `mc-dir-size-{i}`、`mc-render-throttle`）。
- **线程数 ↔ 速度曲线**：实测不同 `WALK_THREADS` 与 dir-size 线程数下的扫描墙钟时间（复用 `cargo bench -p mc-core` 的 `scan_purge_bench`，Analyze 侧另测）。
- **产物**：一张"CPU 归因 + 线程数↔速度"数据表，作为阶段三决策依据。

### 阶段二：渲染/主线程浪费（零速度风险，无条件做）

不依赖测量结果，纯赚：

- 缓存 `size_desc_order` 与扁平行列表，按变更失效（替代每帧重算）。
- 列表视窗裁剪（对齐 GitHub Issue #4「render_children_list 视口优化」；见 `docs/solutions/design-patterns/render-layer-sort-permutation-indices.md`）。
- 静止无变化时不空转重绘（spinner 节流或跳过 no-op 帧）。
- 主循环批量 drain 事件（替代一事件一 select，让突发合并成一次重建）。
- Analyze 侧把"每条目一次 channel 发送"改为批量合并，降低发送端与接收端双侧开销。

### 阶段三：据数据右调扫描并发

- 若阶段一确认扫描 I/O 密集：下调线程数 / 进一步降 I/O 优先级，目标 CPU 大降、速度基本不变。
- 若确认 CPU 密集：保持并发，不牺牲速度；本阶段可能"无操作"。
- 决策必须落在阶段一的数据上，任何调整需实测速度不退化（或退化在阈值内）。

## 成功标准（可验证）

- **CPU**：先实测建立改前基线（当前 150-200%），改后扫描期进程 CPU 显著下降。具体目标百分比在基线测出后敲定。
- **速度**：扫描墙钟时间相对基线不退化，阈值待定（建议 ≤5%）。
- **静止态**：维持近零 CPU，不得回退。
- 全部以活动监视器进程 CPU% + 墙钟计时为准，改前/改后各测一次。

## 开放问题 / 假设

- **假设扫描主要 I/O 密集**（故减线程可省 CPU 不损速）——**必须由阶段一剖析证实或推翻**，是整个阶段三成立与否的前提。
- **主线程事件处理占 150-200% 的多少未知**——决定"批量 drain / 树构建移出主线程"的优先级；若主线程占比大，值得考虑把 `integrate_entry` 移出主线程（更大改动，视数据再定）。
- **具体 CPU 目标值与速度退化阈值**，待改前基线测出后敲定。
- macOS `event::poll(50ms)` 常驻轮询（`crates/tui/src/event.rs:20`）是否值得改为阻塞读——极小项，静止态唯一残留唤醒源，可顺带处理。

## 相关沉淀

- `docs/solutions/design-patterns/render-layer-sort-permutation-indices.md` — 渲染层排序置换模式；已把"每帧全量排序"记为架构问题，指出视口内置换（Issue #4）为下一步。
- `docs/ideation/2026-06-03-p1-perf-ux-ideation.md` — 早期 P1 性能/体验 ideation。
- `docs/ideation/2026-07-04-tui-ux-maturity.md` — TUI 主循环与节流现状记录。
