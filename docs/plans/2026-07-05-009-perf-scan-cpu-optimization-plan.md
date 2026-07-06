---
title: "perf: 扫描期 CPU 占用优化（测量驱动）"
date: 2026-07-05
status: active
type: perf
origin: docs/brainstorms/2026-07-05-cpu-usage-optimization-requirements.md
---

# perf: 扫描期 CPU 占用优化（测量驱动）

## Summary

交互式使用 `mc`（TUI）时，扫描期活动监视器观察到进程 CPU **稳定 150-200%**（几个命令都差不多，扫完才降），与"清理工具本身应轻量"（`STRATEGY.md`）的定位不符。本计划以**测量驱动**方式降低扫描期 CPU，同时保住扫描墙钟时间（`STRATEGY.md` 硬指标"全盘 < 30s"）。

调研已核实：150-200% 超过 1.5-2 个核，**主体是多线程后台扫描池**（Clean 3 遍历线程；Purge 3 遍历 + 4 目录大小线程），渲染是单主线程、已节流，属次要。并进一步定位到一个 **CPU 放大真凶**：Purge 的 `dir_size` 在 4 线程 `par_iter` 内每个匹配目录都 `RayonNewPool(3)` **新建一个 3 线程池**（macOS 特有），峰值 ~16 walker 线程 + 反复建池销毁。

计划分三阶段共 7 个实现单元（U-ID 稳定）：**阶段 A 测量归因** → **阶段 B 渲染/主线程浪费**（零速度风险）→ **阶段 C 据数据右调扫描并发**。测量的"执行"归 `ce-work`，本计划定方法与交付物。

---

## Problem Frame

- **现象**：扫描期 TUI 进程 CPU 150-200%，持续整个扫描，几个命令一致。
- **已核实归因**（见 origin 与本计划 Sources）：
  - 主体 = 多线程扫描池（`crates/core/src/scanner.rs`）。
  - 放大真凶 = 嵌套 rayon 线程池：`dir_size`（`crates/core/src/scanner.rs:498`）内 `create_walker` 用 `RayonNewPool(3)`，而 `dir_size` 本身在 `build_dir_size_pool`（4 线程）的 `par_iter` 里被调（`crates/core/src/scanner.rs:329`、`crates/core/src/scanner.rs:414`）→ 每匹配目录建一个新 3 线程池（jwalk 0.8.1 `RayonNewPool` 语义 = 每次 new walker 建全新池、不复用）。
  - 渲染次要浪费：`size_desc_order` 每帧全量排序（`crates/tui/src/ui/analyzer.rs:21`）、Results 列表每帧 `build_flat_rows` + 全量建 ListItem 不做视窗裁剪（`crates/tui/src/ui/rows.rs:25`、`crates/tui/src/ui/rows.rs:52`）、动画态 Timeout 空转重绘（`crates/tui/src/lib.rs:222`）。
- **静止态**（Menu/Results/Analyzing 浏览/Done）已近零 CPU（主循环纯阻塞 `crates/tui/src/lib.rs:231`），不得回退。

---

## Requirements

- **R1**：扫描期进程 CPU 从 150-200% 显著下降。以活动监视器进程 CPU% 为准，改前/改后各测一次。
- **R2**：扫描墙钟时间相对基线不退化（建议阈值 ≤5%）。以 release 构建实测为准。
- **R3**：静止态维持近零 CPU，不得回退。
- **R4（硬约束·不变错）**：任何 `Found` 事件流改动须守 `(category, base_path)` 按 `PathBuf` 的 delta 累加不变式（见 `docs/solutions/design-patterns/streaming-aggregation-key-is-action-granularity.md`）；回归断言 **path 集合**而非仅 size 求和。
- **R5**：不改清理规则、`SafetyLevel` 分级、删除语义；`dir_size` 大小语义不变（不跟随符号链接等，见 `crates/core/src/scanner.rs` 测试契约）。
- **R6**：不引入新 `unsafe`（`unsafe_code = "deny"`，唯一例外是 `scanner.rs` 的 `setiopolicy_np`）。

---

## Key Technical Decisions

### KTD1 — 测量必须 release，基准改合成树 + 并发可注入
debug 下 jwalk/排序热路径的 CPU% 失真（`docs/solutions/tooling-decisions/rust-workspace-pedantic-clippy-and-release-profile.md`）。现有 `crates/core/benches/scan_purge_bench.rs` 用真实 home（不可跨机/跨配置复现，`WALK_THREADS`/池线程数为编译期 `const`）。故阶段 A 先把并发参数改为**运行时可注入**（env var，release 可配），并把基准改用**合成 tempfile 目录树**，才能做稳定的"线程数↔速度"对照。

### KTD2 — 优先消除嵌套线程池（两全，先于减线程）
减线程数可能拖慢扫描（若 SSD 热缓存下是 CPU 密集），是**有速度风险**的杠杆。而嵌套池的"每目录建池"纯属开销——用 `jwalk::Parallelism::RayonExistingPool` 传入 `build_dir_size_pool` 的现成池复用，可降 CPU **不损并行度、不损速度**。这是阶段 C 的高确定性首选项，减线程数（U7）只在测量证明 I/O 密集时才做。

### KTD3 — 渲染主赢点是记忆化排序，不是视窗裁剪
Analyzer 列表视窗裁剪**已完成**（GitHub Issue #4，见 `crates/tui/src/ui/analyzer.rs:55` 注释），勿重做。剩余浪费是 `size_desc_order` 每帧对全量 children 重算（视口切片前需全局序选 top-N，裁剪省不掉，**只能记忆化省**）。记忆化须守三不变式（`docs/solutions/design-patterns/render-layer-sort-permutation-indices.md`）：稳定排序、跨系访问走 `.get()`、finalize 后重置 cursor/nav_path。视窗裁剪只对 **Results 列表**（`crates/tui/src/ui/rows.rs`）尚需做。

### KTD4 — 批量 drain 不进主计划（测量门控）
"批量 drain（Found/Analyze 事件）"历史已否决（`docs/ideation/2026-06-04-analyzer-interaction-redesign.md` 拒绝表 #8「被 Throttle 方案包含」），且"一事件一 select"是刻意对齐 dua-cli（`crates/tui/src/lib.rs:117`）。仅当阶段 A 测量证明**逐条 integrate 本身是 CPU 热点**（而非重绘）才做；否则不做。列入 Scope Boundaries 的延后项。

### KTD5 — throttle 参数回调是零成本第一杠杆
throttle 从 250ms→200ms→现 80ms（`crates/tui/src/lib.rs:107`）演变得更激进。若阶段 A 归因指向重绘频率，调大 throttle 是零速度代价的首选。空转重绘门控须分态：静态态已阻塞不空转，动画态 Timeout 重绘多服务 spinner——先量清"有无实际状态变化却重绘"，避免砍掉 spinner 需要的 tick。

---

## Implementation Units

### U1. 扫描并发参数可注入 + 合成基准树
**Goal**：让 `WALK_THREADS` 与 dir_size 池线程数可在运行时注入，并把 `scan_purge_bench` 改用合成 tempfile 树，从而能做稳定的线程数↔速度对照。
**Requirements**：R1, R2, KTD1
**Dependencies**：无
**Files**：
- `crates/core/src/scanner.rs`（`WALK_THREADS` const → 运行时读取，如 `MC_WALK_THREADS` env；`build_dir_size_pool` 的 `num_threads(4)` → 可注入，如 `MC_DIRSIZE_THREADS`）
- `crates/core/benches/scan_purge_bench.rs`（改用 tempfile 造固定层数/文件数的合成树，替代 `dirs::home_dir()`；对多组线程数各测一遍）
**Approach**：env 缺省时保持现值（macOS walk=3、dirsize=4），行为不变；仅测量/调参时覆盖。合成树规模需足够触发多匹配目录 + 并行 dir_size（如 N 个含 `package.json` 的 `node_modules`，各若干文件）。
**Patterns to follow**：现有 `build_dir_size_pool`（`crates/core/src/scanner.rs:524`）；测试里 tempfile 造树的方式（`crates/core/src/scanner.rs:617` 起）。
**Test scenarios**：
- env 未设时线程数等于现有缺省（walk=3/dirsize=4 on macOS）。
- env 设为 1/2/6 时 walker 与池线程数相应变化（可通过池 `current_num_threads()` 或行为断言）。
- 合成树基准可运行且结果稳定（同配置多次 sample_size 方差可接受）。
- `Test expectation`：并发数变化不改变扫描**结果**（复用既有 `test_scan_purge_finds_node_modules_and_venv` 断言在不同线程数下仍绿）。
**Verification**：`cargo bench -p mc-core` 在 `MC_WALK_THREADS`/`MC_DIRSIZE_THREADS` 不同取值下产出可比墙钟数字。

### U2. CPU 归因测量 + 数据表产出
**Goal**：产出决策所需的"CPU 归因 + 线程数↔速度"数据表，作为阶段 C 的依据。
**Requirements**：R1, R2, KTD1, KTD2, KTD4, KTD5
**Dependencies**：U1
**Files**：无源码改动（测量执行归 `ce-work`）；产物落 `docs/solutions/` 或 PR 描述。
**Approach**：
- **CPU 归因**：release 构建 TUI（`cargo build --release`），跑真实 Analyze/Clean/Purge，用 macOS `sample <pid>` 或 Instruments Time Profiler 按**线程名**（`mc-dir-size-*`、`mc-render-throttle`、jwalk 线程、主线程）归因 150-200% 的构成。
- **线程数↔速度**：用 U1 的合成基准扫 `MC_WALK_THREADS`/`MC_DIRSIZE_THREADS` ∈ {1,2,3,4,6}，记墙钟。
- **判定**：扫描是 I/O 密集还是 CPU 密集（减线程墙钟是否等比上升）；主线程 integrate 是否显著（决定 KTD4 是否解冻）；重绘频率对 CPU 的贡献（决定 KTD5 的 throttle 调整幅度）。
**Execution note**：这是测量执行单元，交付物是数据表 + 结论，不是代码。
**Test scenarios**：`Test expectation: none -- 测量单元，无行为改动`。
**Verification**：数据表能明确回答上述三个判定问题，且给出 U5/U7 的具体参数取值。

### U3. 记忆化 size_desc_order
**Goal**：消除 Analyzer 每帧对全量 children 的 O(n log n) 重排。
**Requirements**：R1, R3, KTD3
**Dependencies**：无（可与 U1 并行）
**Files**：
- `crates/tui/src/ui/analyzer.rs`（`size_desc_order` 记忆化，或调用方缓存置换）
- `crates/tui/src/app.rs`（若缓存挂 `App`：新增缓存字段 + 失效标志，`App::new` 初始化）
**Approach**：按 children 变更（收到新 `AnalyzeEvent::Entry`/`Found`）或 throttle tick 失效重算，而非每帧。**实时态几乎每事件失效——收益主要在稳定 `Analyzing` 态**；实时态谨慎（不能因缓存导致"跟随最大项"失灵）。守三不变式：稳定排序（等值不抖）、跨系访问走 `.get()`、finalize 后重置 cursor/nav_path。
**Patterns to follow**：`docs/solutions/design-patterns/render-layer-sort-permutation-indices.md`（显示层置换）；现有 finalize 异步化路径（`crates/tui/src/lib.rs:197` SortDone）。
**Test scenarios**：
- 稳定 `Analyzing` 态：连续多帧渲染，`size_desc_order` 只在 children 未变时命中缓存（不重算）。
- children 变更后缓存失效，下一帧顺序正确（按 size 降序）。
- 等值 size 的稳定序：两项 size 相等时顺序不随重算抖动。
- finalize 后 cursor/nav_path 重置到 root，无陈旧坐标越界（走 `.get()` 降为 no-op）。
- 实时态：新增更大项时"跟随最大项"仍生效（缓存不破坏该行为）。
**Verification**：`cargo test -p mc-tui` 绿；tmux 跑 Analyze（`verify-tui` skill）实时列表顺序与跟随行为不回退。

### U4. Results 列表视窗裁剪 + 缓存 build_flat_rows
**Goal**：Results/Scanning 列表只为可见区间建 ListItem，并避免每帧/每按键全量重建 `build_flat_rows`。
**Requirements**：R1, R3
**Dependencies**：无
**Files**：
- `crates/tui/src/ui/rows.rs`（`render_flat_list` 只对 `window_start..window_end` 建 ListItem，对齐 analyzer 的 `render_children_list`）
- `crates/tui/src/app.rs`（`build_flat_rows` 结果缓存 + 脏标志；按 `scan_result`/`expanded`/`filter_query`/`marked` 变更置脏；`App::new` 初始化；调用点 `current_detail`/`clamp_result_cursor` 复用缓存）
**Approach**：缓存字段用安全手段（`Option<Vec<FlatRow>>` + `dirty: bool`，单线程主循环）。视窗裁剪按 `result_cursor` 现有 `scroll_offset` 计算可见区间。
**Patterns to follow**：`crates/tui/src/ui/analyzer.rs:55` 起的视口优化（Issue #4 已落地样板）。
**Test scenarios**：
- 大量分类/项（上千行）时，`render_flat_list` 只构建可见区间数量的 ListItem（可对 `build_flat_rows` 输出切片长度断言）。
- 缓存命中：`scan_result` 未变时连续调用 `build_flat_rows` 返回缓存、不重建。
- 各失效源（新 Found 改 `scan_result`、展开/折叠改 `expanded`、输入 filter、标记/取消标记改 `marked`）触发重建，内容正确。
- 光标滚动到列表尾/头时可见区间正确，无越界 panic（走 `.get()`）。
- 过滤态：`filter_query` 下缓存与视窗协同，显示行数与过滤结果一致。
**Verification**：`cargo test -p mc-tui` 绿；tmux 跑 Clean/Purge 结果列表滚动、展开、过滤、标记均正常。

### U5. throttle 参数回调 + 空转重绘门控（测量门控）
**Goal**：据 U2 数据，若重绘频率是 CPU 贡献者，调整 throttle 并跳过无状态变化的纯 spinner 帧。
**Requirements**：R1, R3, KTD5
**Dependencies**：U2
**Files**：
- `crates/tui/src/lib.rs`（`Throttle::new` 参数 `crates/tui/src/lib.rs:107`；Timeout 分支重绘门控 `crates/tui/src/lib.rs:222`）
- `crates/tui/src/throttle.rs`（若参数化 duration）
**Approach**：分态处理——静态态已阻塞不动；动画态 Timeout 重绘仅在有实际状态变化或 spinner 需要推进时进行。避免砍掉 spinner tick 导致动画冻结。具体 throttle 数值取 U2 结论。
**Test scenarios**：
- 动画态无状态变化时重绘频率受控（不超过 throttle 频率）。
- spinner 仍按预期推进（tick 不被误砍）。
- 静态态零空转（回归 R3）。
- `Covers R3.` 扫描完成回到 Results 静态态后 CPU 归零行为不变。
**Verification**：tmux 观测 spinner 流畅；改前/改后活动监视器 CPU 对照（交付于 U2 数据表框架）。

### U6. 消除嵌套线程池（RayonExistingPool 复用）
**Goal**：Purge 的 `dir_size` 不再每匹配目录新建 3 线程池，改复用 `build_dir_size_pool` 的现成池，降 CPU 不损并行度。
**Requirements**：R1, R2, R5, R6, KTD2
**Dependencies**：无（但改后须用 U1 基准验证速度不退化）
**Files**：
- `crates/core/src/scanner.rs`（`create_walker`/`dir_size` 的 `Parallelism` 从 `RayonNewPool` 改 `RayonExistingPool { pool, busy_timeout }`，传入 dir_size 池的 rayon 池引用；`build_dir_size_pool` 相应暴露/传递池句柄）
**Approach**：评估 `RayonExistingPool` 与 `setiopolicy_np` I/O 优先级降级的协同（现降级挂在池 `start_handler`，复用同一池天然继承）。注意 walk 主遍历（clean/analyze/purge 剪枝遍历）与 dir_size 遍历的池使用边界，避免死锁/饥饿（dir_size 在池内 `install`，其 walker 又用同池——须确认 jwalk `RayonExistingPool` 在嵌套 `install` 下不自锁；若有风险，改为单层：dir_size 不并行 walk、由外层 `par_iter` 提供并行）。
**Technical design**（directional，非实现规格）：
```
现状: dir_size_pool(4).install(|| par_iter(dirs).map(dir_size))
        dir_size -> create_walker -> RayonNewPool(3)   // 每目录新建池
目标: 复用单一池；两条候选：
  (a) dir_size 的 walker 用 RayonExistingPool(dir_size_pool 的 rayon 池)
  (b) dir_size 改单线程遍历，并行度完全由外层 par_iter 提供（更简单，需 U1 基准确认不慢）
  选 (a)/(b) 由 U1 基准数据定。
```
**Patterns to follow**：`crates/core/src/scanner.rs:524` `build_dir_size_pool`；jwalk 0.8.1 `Parallelism` 枚举。
**Test scenarios**：
- `Covers R5.` `test_dir_size_sums_files`、`test_dir_size_empty_dir`、`test_dir_size_nonexistent` 仍绿（大小语义不变）。
- `Covers R5.` `test_symlinks_not_followed`、`test_scan_purge_does_not_descend_into_matched_dirs` 仍绿。
- `test_scan_purge_finds_node_modules_and_venv`、`test_dirname_root_guards_sibling_and_inside`、`test_rust_target_requires_cargo_toml` 仍绿（匹配/守卫不变）。
- 取消：`is_cancelled` 在复用池下仍能及时中止（`dir_size` 内每 1024 entry 检查）。
- 无死锁：多匹配目录（≥8）并行算大小在复用池下正常完成（新增回归）。
**Verification**：`cargo test -p mc-core` 全绿；`cargo bench -p mc-core` 改前/改后墙钟对照满足 R2（不退化）；改后 CPU（活动监视器）较改前下降（U2 框架实测）。

### U7. 据数据右调扫描线程数 / I/O 优先级（测量门控）
**Goal**：若 U2 证明扫描 I/O 密集，下调线程数使 CPU 大降、速度基本不变；若 CPU 密集则保持（本单元可能"无操作"）。
**Requirements**：R1, R2, KTD2
**Dependencies**：U2, U6
**Files**：
- `crates/core/src/scanner.rs`（`WALK_THREADS` 缺省值、`build_dir_size_pool` 缺省线程数；若调 I/O 优先级则改 `setiopolicy_np` 处，保 `#[allow(unsafe_code)]` 注释与理由）
**Approach**：仅据 U2 数据表调缺省值；每次调整用 U1 基准实测速度退化 ≤ R2 阈值才保留。不引入 rayon 到规则扫描（历史否决，收益有限）。对标 dua-cli（`/Users/zhaohejie/workspace/explore/dua-cli`，3 线程/128KB 栈）。
**Test scenarios**：
- `Covers R2.` 新缺省线程数下 `scan_purge_bench` 墙钟相对基线退化 ≤5%。
- 扫描结果不变（既有 purge 测试全绿）。
- `Test expectation`：若数据判定为 CPU 密集导致本单元无操作，则记录结论并跳过，不做无依据改动。
**Verification**：基准墙钟满足 R2；活动监视器 CPU 较基线下降。

---

## Scope Boundaries

### Deferred to Follow-Up Work
- **批量 drain（Found/Analyze 事件合并）**：仅当 U2 证明逐条 integrate 是 CPU 热点才解冻（KTD4）。若做，必须守 R4 的 `(category, base_path)` 按 PathBuf 聚合不变式，回归断言 path 集合。
- **抽 Analyze 遍历到 mc-core 以加 criterion 基准**：当前 Analyze 遍历内联在 `crates/tui/src/lib.rs:551`；U2 用 `sample`/Instruments 归因即可，抽离改由后续（注意别与 GitHub Issue #13 lib.rs 收敛重构撞车）。
- **event.rs 常驻 `event::poll(50ms)` 改阻塞读**（`crates/tui/src/event.rs:20`）：静态态唯一残留唤醒源，极小项，视 U2 数据决定是否顺带。

### 明确不做
- 不改 CLI 的 indicatif 进度路径（高 CPU 是交互态 TUI）。
- 不改清理规则、`SafetyLevel`、删除语义（R5）。
- 不做 GUI 层（尚未存在）。
- 不引入新 `unsafe`（R6）；不引 rayon 到规则扫描。
- 不重构静止态（已近零 CPU）。

---

## Risks & Dependencies

- **R-1 嵌套池改造死锁风险**（U6）：同一池嵌套 `install` + jwalk `RayonExistingPool` 可能自锁/饥饿。缓解：优先候选 (b)（dir_size 单线程、并行度由外层 par_iter 提供），或充分回归 ≥8 目录并行；改前后跑全 `mc-core` 测试。
- **R-2 记忆化破坏实时行为**（U3）：缓存导致"跟随最大项"失灵或顺序抖动。缓解：实时态谨慎失效、稳定排序、tmux 实测。
- **R-3 测量不可复现**（U1/U2）：真实 home 漂移。缓解：合成 tempfile 树 + release 构建 + 同机相对对照。
- **R-4 速度退化**（U6/U7）：并发改动拖慢扫描。缓解：每次改动过 U1 基准，R2 阈值门控。
- **依赖**：U2 门控 U5/U7；U6 独立但须 U1 基准背书；实现全程走分支/worktree（main 禁 Edit/Write，worktree 内 clippy hook 不生效需手动 `cargo clippy --all-targets`）。

---

## Success Metrics

- **CPU**：改前基线 150-200%；改后扫描期进程 CPU 显著下降（具体目标值待 U2 基线敲定）。
- **速度**：`scan_purge_bench` 改后墙钟相对基线退化 ≤5%（R2）。
- **静止态**：维持近零 CPU（R3）。
- **正确性**：`cargo test` 全绿；`cargo clippy --all-targets` 无警告；R4/R5 语义不变。

---

## Sources & Research

- Origin：`docs/brainstorms/2026-07-05-cpu-usage-optimization-requirements.md`
- `docs/solutions/design-patterns/render-layer-sort-permutation-indices.md`（记忆化排序三不变式；Analyzer 视窗裁剪已完成 = Issue #4）
- `docs/solutions/design-patterns/streaming-aggregation-key-is-action-granularity.md`（R4 聚合不变式）
- `docs/ideation/2026-06-04-analyzer-interaction-redesign.md`（批量 drain 拒绝表 #8）
- `docs/ideation/2026-06-03-p1-perf-ux-ideation.md`（线程数决策来源、scan_purge 历史瓶颈已由 plan 005 修）
- `docs/ideation/2026-07-04-tui-ux-maturity.md`（throttle 250→200→80ms 演变）
- `docs/solutions/tooling-decisions/rust-workspace-pedantic-clippy-and-release-profile.md`（release 测量前提、unsafe/lint 约束）
- jwalk 0.8.1 `Parallelism::RayonNewPool`/`RayonExistingPool` 语义（源码核实：每 walker 新建池）
- dua-cli 并发对标：`/Users/zhaohejie/workspace/explore/dua-cli`（3 线程/128KB 栈/bounded channel）

---

## 实现结果（2026-07-05）

**本 PR 交付**：U1（并发 env 可注入 `MC_WALK_THREADS`/`MC_DIRSIZE_THREADS` + `scan_purge_bench` 改合成 tempfile 树）、U6（`dir_size` 改串行 walker，消除"每匹配目录新建 3 线程池"的嵌套池 churn）、U4（Results/扫描列表视口裁剪，只为可见行建 ListItem）。

**测量结论（U2，受控 A/B：`~/workspace` dry-run，release，暖缓存、交替跑 3 轮、同机同目录）**：

| | 墙钟 (real) 均值 | user+sys = CPU 秒 均值 | avg CPU% |
|---|---|---|---|
| OLD（嵌套池 ~16 线程） | 6.53s | **~20.4** | **~312%** |
| NEW（U6 串行 dir_size / 4） | 6.65s | **~7.2** | **~108%** |

三轮原始数据（real / user / sys）：
- OLD：6.71/4.50/15.11 · 6.23/5.25/15.42 · 6.64/5.16/15.62
- NEW：6.82/1.37/5.64 · 6.05/1.37/5.72 · 7.07/1.40/6.00

- **CPU 占用降 ~2.8×（−65%）**，avg CPU 从 ~312%（吻合用户观察的 150-200%+）降到 ~108%（约 1 个核）——达成 **R1**。
- **扫描速度无退化**：墙钟 6.53s→6.65s（**+1.8%**，噪声内、远低于 ≤5% 红线），受控暖缓存测量已排除缓存偏差——达成 **R2**。合成基准 criterion 20 样本亦为 −3.5%、p=0.53（不显著）。
- **冒烟点在 sys（内核态）**：OLD sys ~15.4s vs NEW ~5.8s。老版每匹配目录新建/销毁 3 线程池 + ~16 线程互抢，CPU 烧在线程创建与上下文切换（内核开销），不产出扫描进度；串行遍历砍掉的正是这部分空耗。

**据数据延后 / 判定不做**：

- **U5（throttle/空转重绘）**：U6 已把总 CPU 降到约 1 个核，渲染仅占其中一小片，调 throttle 收益不足以 justify——延后，除非后续实测重绘仍是热点。
- **U7（减扫描线程数）**：U6 已达标且墙钟未退化；进一步减外层线程只会为无谓的 CPU 需求冒墙钟退化风险——**判定不做**，保持缺省（外层 4 / 内层串行）。`MC_DIRSIZE_THREADS` 已留作后续调参旋钮。
- **U3（记忆化 size_desc_order）**：仅影响 AnalyzingLive 当前节点、已被 80ms 节流、实时态几乎每事件失效，收益边际且有置换缓存陈旧风险；U6 达标后不再必要——延后待测量证明渲染是热点再做。
- **批量 drain**：历史已否决（拒绝表 #8），本次测量未显示逐条 integrate 是热点，维持延后。

---

## 下一步优化结论（若还想进一步降 CPU）

本 PR 后，扫描期 CPU 已收敛到 **约 1 个核（~108%）**，且其中 **~5.8s 是 sys（文件系统 syscall）**——这是遍历磁盘的**固有成本**，不再是并发/渲染的浪费。对一个"扫全盘找垃圾"的工具，1 核占用是合理量级。因此**进一步优化收益递减，非必要不做**。若确有需求，按性价比排序、且都需先测量证明是热点：

1. **减少 metadata syscall（最大的剩余 CPU 来源）**：`prefetch_metadata` 对每个文件调 `.metadata()`（一次 stat）。当前只在需要大小的地方调（`dir_size`、Clean/Analyze 遍历），Purge 剪枝遍历已不调——已较克制。可探索的方向：Purge 场景其实只需**目录**总大小，若某些规则不需要逐文件明细，可省去部分 stat；但需权衡"流式逐项证据文案"的展示需求，收益不确定。
2. **大单目录的墙钟**（如 20GB 级 target/DerivedData）：串行 walker 单线程遍历，理论上比老版内层 3 线程慢；实测墙钟未见退化（被众多小目录的并行摊平），故**暂不处理**。若未来出现"单个超大目录扫描慢"的反馈，可给 `dir_size` 接一个**共享持久 walk 池**（`RayonExistingPool` 传入独立于 `par_iter` 的池，避免同池 self-lock），恢复大目录的内层并行而不重蹈每目录建池。
3. **渲染层（U3/U4 剩余）**：仅当交互实测显示主线程渲染成为热点时才做记忆化排序 / `build_flat_rows` 缓存。当前渲染只是 1 核里的一小片，不值得。
4. **CLI indicatif 路径**：本次未测，若 CLI（非 TUI）也报 CPU 问题再单独归因。

**一句话**：CPU 已从"浪费主导"转为"固有 fs 成本主导"，本轮优化到此为收益拐点；后续任何动作都应先用本 PR 留下的 `MC_*_THREADS` env 旋钮 + `scan_purge_bench` 合成基准做测量,再决定。

> **后续修正（issue #20 / plan 010）**：上面"已到收益拐点"的结论**被推翻**。更深归因（`sample` + 受控 A/B）
> 发现剩余 CPU 里 ~25–40% 仍是**可控自旋**——jwalk 并行迭代器消费端的固定 `sched_yield` 忙等，与池大小无关。
> 已由**自写 park 式阻塞遍历器**（`MC_WALK_ENGINE=park`）消除：冻结树 CPU 秒 −51%、`swtch_pri` 顶栈占比
> 40.1%→0.6%、墙钟不损、结果逐字节一致。另有一半高 CPU 是本机阿里 EDR 的 close 税（环境因素、不代表真实用户，
> 见 [[../solutions/tooling-decisions/edr-syscall-tax-distorts-cpu-measurement]]）。详见
> `docs/plans/2026-07-05-010-perf-park-walker-scan-engine-plan.md`。
