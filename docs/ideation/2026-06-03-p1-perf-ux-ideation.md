---
date: 2026-06-03
topic: p1-perf-ux-optimization
focus: P1 性能与体验优化
mode: repo-grounded
---

# Ideation: P1 性能与体验优化

## Grounding Context

**项目形态:** Rust workspace（core/cli/tui），使用 jwalk 并行遍历、ratatui+crossterm TUI、crossbeam-channel 事件通信。rayon 已声明但未使用。

**已知性能瓶颈:**
1. `scan_purge_dir` 先 jwalk 收集匹配目录，再对每个串行调用 `dir_size()` 做完整重新遍历
2. `scan_with_rules` 对每条 Exact 规则独立启动 jwalk，Library/Caches 被遍历 4 次
3. Analyzer 每次 Enter 进入子目录都 spawn 新线程全量 jwalk 重扫
4. TUI 每 200ms 无条件 Tick + `terminal.draw()`，静态页面白白重绘
5. stdout 未用 BufWriter，每帧产生数百次 write 系统调用
6. 扫描中不响应任何按键，无法取消
7. 无进度百分比/ETA，用户无法判断还要等多久

**dua-cli 源码关键参考（/Users/zhaohejie/workspace/explore/dua-cli）:**
- `petgraph::StableGraph` 存树（因需 stable 删除），单遍深度栈聚合 size
- 后台线程 + `crossbeam::channel::bounded(100)` 通信
- `crossbeam::select!` 多路复用键盘+遍历事件，无 Tick 线程
- macOS 限制 3 线程（`options.rs:39-44`），128KB 线程栈
- channel 关闭即取消（无 AtomicBool）
- `process_read_dir` 回调中收集 metadata 避免二次 stat
- `std::mem::forget(app)` 避免退出时 dealloc 延迟

**与 dua-cli 的本质差异（我们是清理工具，不是磁盘分析器）:**
- 多扫描模式（Clean/Purge/Uninstall/Analyze），只有 Analyze 类似 dua
- 扫描需同时匹配规则（DirName + Exact + Cargo.toml 验证）
- 结果按 category + SafetyLevel 分组，不是一棵统一的树
- CLI + TUI 双模式
- 标记基于 `ScanItem.selected` 布尔值 + SafetyLevel 自动选中

## Topic Axes

1. 扫描引擎性能 — jwalk 遍历策略、dir_size 消除、规则批处理、线程调优
2. TUI 渲染与响应 — 帧率控制、事件驱动渲染、BufWriter 缓冲
3. 扫描-UI 通信架构 — channel 设计、渐进式结果、取消机制
4. 交互反馈与导航 — 进度指示、Analyzer 缓存、键盘导航流畅度

## Ranked Ideas

### 1. 单遍遍历引擎 — 消除 dir_size 二次遍历
**Description:** 反转 `scan_purge_dir` 的"先找再量"两遍模式为"边走边量"。在 `process_read_dir` 回调匹配到目录后，不剪枝退出，继续深入遍历并将文件 size 累加到匹配目录的计数器上。整个 purge 扫描只需一次 jwalk 遍历。比 dua 更复杂：dua 只累加 size，我们需要同时做 DirName 规则匹配 + Cargo.toml 验证 + size 累加。参考 dua 的深度栈模式（`directory_info_per_depth_level`）做 size 聚合。
**Axis:** 扫描引擎性能
**Basis:** `direct:` `scanner.rs:238-249` — N 个匹配目录串行 `dir_size()` 是 purge 扫描最大瓶颈。`external:` dua-cli `traverse.rs:370-423` 用深度栈单遍聚合，已验证此模式在 jwalk 上可行。
**Rationale:** 20 个 node_modules = 20 次串行 jwalk 重新遍历。单遍后 purge 扫描时间可缩短 50-80%。
**Downsides:** `process_read_dir` 回调中规则匹配 + size 累加的并发安全性需仔细处理（dua 用 Arc<Mutex>）。代码复杂度增加。
**Confidence:** 85%
**Complexity:** Medium
**Status:** Unexplored

### 2. 规则路径合并 — 消除重叠路径的冗余遍历
**Description:** `scan_with_rules` 对 6 条 clean 规则的 Exact 路径独立启动 jwalk。其中 `~/Library/Caches` 和 `~/Library/Caches/Google/Chrome` 存在包含关系，同一目录树被遍历 4 次。扫描前对所有 Exact 路径做树形合并：如果 A 是 B 的祖先，只遍历 A，在遍历过程中按路径前缀将文件分发到对应规则的 category。
**Axis:** 扫描引擎性能
**Basis:** `direct:` `rules.rs:34,58` — Library/Caches 和 Chrome Cache 路径嵌套；`scanner.rs:46` 对每个 Exact 路径独立 jwalk。
**Rationale:** Library/Caches 通常 5-20GB、数十万文件。4 次遍历 → 1 次，clean 命令速度提升 2-3 倍。
**Downsides:** 同一目录可能被不同规则以不同 SafetyLevel 匹配，需要在分发时选择正确的 category 和 safety。
**Confidence:** 80%
**Complexity:** Medium
**Status:** Unexplored

### 3. Analyzer 缓存树 + 惰性展开
**Description:** 首次 `build_dir_tree` 时递归构建完整多层树结构（而非只保留 depth=1），缓存在 DirNode 中。Enter 操作变成纯内存视图切换。导航使用 `Vec<usize>` 索引路径（借鉴 dua 的 bookmark 模式）替代 `std::mem::replace` 移动整棵树。不需要 petgraph——我们的 Analyzer 不做节点删除（标记删除走 Cleaner），递归 DirNode 缓存足够。
**Axis:** 交互反馈与导航
**Basis:** `direct:` `lib.rs:556-561` — 每次 Enter spawn 新线程全量 jwalk。用户反馈"非常慢"。`external:` dua-cli `navigation.rs` 用内存图遍历 + bookmark 实现即时导航。
**Rationale:** 用户反馈"卡死"和"非常慢"的直接原因。改为缓存后导航 <1ms。
**Downsides:** 大目录（~/）的完整树可能消耗较多内存。可通过 max_depth 限制缓解。
**Confidence:** 90%
**Complexity:** Medium
**Status:** Unexplored

### 4. 事件驱动渲染 + BufWriter（组合快赢）
**Description:** 两个改动打包：(a) 去掉 Tick 线程，改用 `crossbeam::select!` 多路复用键盘事件和进度事件（借鉴 dua `eventloop.rs:128-194`），仅在收到事件时渲染。Scanning/Cleaning 状态下可用 `crossterm::event::poll(33ms)` 超时驱动 spinner 动画。(b) 用 `BufWriter::new(stdout())` 包装 CrosstermBackend。比最初提的 dirty-flag 更彻底。
**Axis:** TUI 渲染与响应
**Basis:** `direct:` `event.rs:49-56` — 200ms 无条件 Tick；`lib.rs:39` — 裸 stdout。`external:` dua-cli 无 Tick 线程，纯事件驱动 via `crossbeam::select!`。BufWriter 减少 100x write 系统调用。
**Rationale:** 静态页面 CPU 占用降至 ~0%。渲染帧时间降低数倍。两处改动加起来改动量小但效果显著。
**Downsides:** 需要重构 EventHandler 架构。Scanning 状态下的 spinner 动画需要额外处理（poll 超时或定时 channel）。
**Confidence:** 90%
**Complexity:** Medium
**Status:** Unexplored

### 5. 流式结果展示 — 边扫描边浏览
**Description:** 将 Results 页和 Scanning 页合并为"流式结果"视图——收到第一个 Found 事件后即可进入结果浏览模式，用户边浏览已发现的分类、边等后续结果流入。顶部保持扫描进度指示。代码中 `handle_progress` 已在扫描阶段增量构建 `app.scan_result`，数据已就绪，只是 UI 状态机人为延迟展示。
**Axis:** 扫描-UI 通信架构
**Basis:** `direct:` `lib.rs:278-297` — `ProgressEvent::Found` 分支已增量构建 scan_result，但被 `Complete` 事件守门。
**Rationale:** 5 条规则第一条 1 秒扫完、最后一条 10 秒。流式模式下 1 秒后即可操作。感知等待从"最慢规则"变为"最快规则"。
**Downsides:** 用户在扫描未完成时确认清理可能遗漏后续项目。需明确"仍在扫描"提示和保护逻辑。
**Confidence:** 75%
**Complexity:** Medium
**Status:** Unexplored

### 6. 扫描取消支持 — channel 关闭即取消
**Description:** 借鉴 dua 的取消模式：不使用 AtomicBool，而是利用 bounded channel 的关闭语义。UI 端收到 Esc 后 drop progress_rx（或 stop consuming），scanner 线程的 `progress_tx.send()` 返回 Err 即退出 jwalk 遍历。底部提示从"请等待扫描完成..."改为"Esc 取消 | 请等待扫描完成..."。
**Axis:** 扫描-UI 通信架构
**Basis:** `direct:` `lib.rs:108-109` — Scanning 状态不响应按键。`external:` dua-cli `traverse.rs:257-260` 用 `entry_tx.send().is_err()` 检测取消，无额外状态变量。
**Rationale:** 用户误触 Analyze 后被困在无法取消的等待中。channel 关闭比 AtomicBool 更简洁（零额外状态）。
**Downsides:** 需要确保 scanner 线程在 channel 关闭后能快速退出（jwalk 的 process_read_dir 回调中也需要检查）。
**Confidence:** 90%
**Complexity:** Low
**Status:** Unexplored

### 7. 规则级进度指示 + macOS 线程调优
**Description:** 两个低成本改进打包：(a) ProgressEvent 增加 `RuleProgress { current, total, name }` 变体，每完成一条规则发送。TUI 显示 `[3/6] 浏览器缓存`。(b) macOS 上限制 jwalk 线程数为 3（借鉴 dua 的 `DEFAULT_THREADS`），线程栈 128KB。dua 作者在 M4 Mac 实测 3 线程最优。
**Axis:** 交互反馈与导航 + 扫描引擎性能
**Basis:** `direct:` `scan.rs:55-88` — 无百分比；`scanner.rs:35-107` — 规则数已知。`external:` dua-cli `options.rs:39-44` — macOS 3 线程最优。
**Rationale:** 进度条让等待可预期（Nielsen #1）。3 线程减少 APFS 上的锁竞争。两个都是零风险改动。
**Downsides:** Purge 模式进度估算不如 Clean 精确。3 线程限制可能不适用于所有 Mac 型号（旧款 HDD 可能需要更少）。
**Confidence:** 90%
**Complexity:** Low
**Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | 并行规则扫描（rayon par_iter） | 与"规则路径合并"重叠；合并后可并行空间有限 |
| 2 | Arena 树（petgraph/indextree） | P1 过重；递归 DirNode 缓存已解决即时痛点。我们不做 stable 删除，不需要 petgraph |
| 3 | 扫描结果持久化缓存（FSEvents + 增量） | 基础设施量大，属 P2 |
| 4 | Results→Analyzer 快捷跳转 | UX 锦上添花，非 P1 性能核心 |
| 5 | 双向 scan-UI channel | 架构复杂度过高，scope overrun |
| 6 | 后台守护进程（launchd daemon） | scope overrun |
| 7 | HashSet 规则匹配 | 当前 ~10 条规则，过早优化 |
| 8 | 自适应进度汇报频率 | 与 bounded channel 重叠 |
| 9 | Event Sourcing 架构 | 架构重构，scope overrun |
| 10 | core::fs 统一模块 | P2 代码质量，非 P1 性能 |
| 11 | 扫描→Arena→视图 流水线 | P2 架构重构 |
| 12 | 估算先行精确后补 | 单遍遍历消除 dir_size 后不再需要 |
| 13 | Preflight 预估（statvfs） | 与规则级进度重叠，边际收益 |
| 14 | Top-N 惰性分析器 | 人为截断违反用户预期 |
| 15 | Bounded channel（独立提案） | 独立影响小；已融入 #4 和 #6 的实现 |
| 16 | 增量感知扫描（FSEvents） | 基础设施量大，P2 |

## Open Product Questions

### Analyzer 功能需要产品重设计

**问题:** 当前 Analyzer 是一个简化版 dua-cli（磁盘目录浏览），对用户没有增量价值——用户要通用磁盘分析直接装 dua-cli/ncdu。macCleaner 的核心价值是"智能清理"（Clean/Purge/Uninstall），而非磁盘分析。

**方向:** 让 Analyzer 服务于清理，而非做通用磁盘浏览。具体方案待 `/ce-brainstorm` 探讨，可能包括：
- 与 SafetyLevel 联动（标注哪些目录可以安全清理）
- 与已有规则联动（高亮规则已覆盖的目录）
- 发现规则没覆盖到的可清理空间
- 直接支持标记删除（当前 Analyzer 无法操作）

**当前处理:** P1 只修 Analyzer 的缓存/渲染性能（Idea #3、#4），不改产品逻辑。产品重设计作为独立任务后续推进。

## dua-cli 源码参考

基于对 `/Users/zhaohejie/workspace/explore/dua-cli` 源码的深入阅读，以下模式可借鉴但需适配清理场景：

### 可直接借鉴
- **事件驱动渲染**：`crossbeam::select!` 多路复用键盘+遍历事件，无 Tick 线程（`eventloop.rs:128-194`）
- **bounded channel(100)** + 发送失败即取消（`traverse.rs:257-260`）
- **macOS 3 线程**（`options.rs:39-44`），128KB 线程栈（`common.rs:240-254`）
- **深度栈单遍聚合**（`traverse.rs:370-423`）

### 需适配清理场景
- **petgraph::StableGraph** → 我们不需要，Analyzer 不做节点删除，递归 DirNode 缓存足够
- **单遍聚合** → dua 只累加 size，我们需同时匹配规则（DirName + Exact + Cargo.toml 验证）
- **树结构** → Clean/Purge 结果是按 category 分组的 ScanResult，不是统一的树
- **标记删除** → dua 用 BTreeMap<NodeIndex, EntryMark>，我们用 ScanItem.selected + SafetyLevel
