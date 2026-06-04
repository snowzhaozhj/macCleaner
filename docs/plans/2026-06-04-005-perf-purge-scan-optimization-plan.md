---
title: "perf: Purge 扫描性能 benchmark + 优化"
status: active
origin: docs/brainstorms/2026-06-04-purge-scan-perf-requirements.md
date: 2026-06-04
depth: standard
---

# perf: Purge 扫描性能 benchmark + 优化

## Summary

为 purge 扫描建立 criterion benchmark 基线，然后实施三个优化：并行化 Exact 规则的 dir_size 计算、为 purge 模式扩展跳过目录列表、减少遍历阶段的 mutex 锁竞争。目标：`mc purge ~` < 30s。

---

## Problem Frame

当前 purge 扫描 `~` 耗时 > 30s，超出 STRATEGY.md 定义的性能目标。瓶颈分析：
1. Exact 规则的 `dir_size()` 串行执行（Docker 64GB 目录的遍历本身就耗时）
2. jwalk 遍历 `~` 时进入了 `Library`、`Applications` 等不可能有开发产物的大目录
3. 遍历阶段每个文件都对 `matched_dirs` Mutex 加锁做 starts_with 查找

(see origin: `docs/brainstorms/2026-06-04-purge-scan-perf-requirements.md`)

---

## Requirements

- R1: criterion benchmark 覆盖 `scan_purge_dir`
- R2: Exact 规则 dir_size 并行计算（rayon）
- R3: purge 模式跳过 Library/Applications/.Trash/Pictures/Music/Movies
- R4: 优化后 benchmark 对比 + 实测 < 30s

---

## Key Technical Decisions

1. **Benchmark 使用 criterion** — 项目已有 rayon 依赖，criterion 是 Rust 社区标准 bench 框架，支持统计分析和回归检测
2. **Exact dir_size 用 rayon par_iter 并行化** — 10 条 Exact 规则独立无依赖，天然可并行；rayon 已在 workspace dependencies 中
3. **SKIP_DIRS 改为 purge-specific 参数** — 不同扫描模式跳过不同目录，避免影响 clean/analyze 的行为
4. **不改 matched_dirs 的 Mutex 架构** — 锁竞争是次要瓶颈（相比 I/O），且改为 lock-free 结构风险较高。优先做跳过目录来减少进入锁的文件数

---

## Scope Boundaries

### 不做
- 不做增量缓存
- 不改 jwalk 线程数配置
- 不优化 clean/analyze 命令
- 不改 matched_dirs 为 lock-free 结构

### Deferred to Follow-Up Work
- 增量缓存（inotify/fsevents watch）
- 更细粒度的 dir_size 进度报告

---

## Implementation Units

### U1. 添加 criterion benchmark

**Goal:** 建立 scan_purge_dir 的可复现性能基线

**Requirements:** R1

**Dependencies:** 无

**Files:**
- `Cargo.toml` — 添加 criterion dev-dependency
- `crates/core/Cargo.toml` — 添加 criterion dev-dependency + [[bench]] 配置
- `crates/core/benches/scan_purge_bench.rs` — benchmark 实现

**Approach:**
- 用 criterion 的 `criterion_group!` 宏定义 benchmark
- benchmark 对用户 home 目录下的实际 workspace 路径执行 scan_purge_dir（使用 NullReporter 忽略 progress events）
- 记录 wall-clock 时间作为基线

**Patterns to follow:** criterion 标准用法，参考 `crates/core/Cargo.toml` 已有的 dev-dependencies 结构

**Test scenarios:**
- `cargo bench -- scan_purge` 能运行并输出时间统计（mean、std deviation）
- benchmark 不 panic，不依赖特定目录存在

**Verification:** `cargo bench` 输出包含 scan_purge benchmark 结果

---

### U2. 并行化 Exact 规则的 dir_size 计算

**Goal:** 10 条 Exact 规则的目录大小计算从串行改为 rayon 并行

**Requirements:** R2

**Dependencies:** U1（有基线可对比）

**Files:**
- `crates/core/src/scanner.rs` — 修改 `scan_purge_dir` 中 Exact 规则处理部分

**Approach:**
- 收集所有符合条件的 Exact 路径到 Vec
- 用 `rayon::par_iter()` 并行计算每个路径的 dir_size
- 并行完成后串行发送 ProgressEvent::Found（reporter 不是 Send）
- 保持现有的 `exact_path.starts_with(base_path)` 检查

**Patterns to follow:** 项目已有 rayon 使用（`crates/core/Cargo.toml`），参考 `create_walker` 中的并行模式

**Test scenarios:**
- 多个 Exact 路径存在时，所有路径都被正确扫描并报告大小
- Exact 路径不存在时被跳过，不 panic
- 扫描结果与串行版本一致（大小相同）

**Verification:** benchmark 对比 U1 基线，Exact 阶段耗时应显著降低

---

### U3. 扩展 purge 模式跳过目录列表

**Goal:** purge 遍历时跳过不可能包含开发产物的大目录

**Requirements:** R3

**Dependencies:** U1

**Files:**
- `crates/core/src/scanner.rs` — 修改 `scan_purge_dir` 的 `process_read_dir` 回调

**Approach:**
- 新增 `PURGE_SKIP_DIRS` 常量，包含 `Library`、`Applications`、`.Trash`、`Pictures`、`Music`、`Movies`（加上现有的 `.git`、`.Spotlight-V100`、`.fseventsd`）
- 在 purge walker 的 `process_read_dir` 中使用 `PURGE_SKIP_DIRS` 替代全局 `SKIP_DIRS`
- 全局 `SKIP_DIRS` 保持不变（供 clean/analyze 使用）

**Patterns to follow:** 现有 `SKIP_DIRS` 的使用模式（`crates/core/src/scanner.rs:9`）

**Test scenarios:**
- purge 扫描 ~ 时不进入 Library 目录（通过检查扫描结果不包含 Library 下的文件验证）
- purge 扫描 ~/workspace 时正常工作（workspace 不在跳过列表中）
- clean 模式的 SKIP_DIRS 未被影响（clean 仍然能扫描 Library/Caches）

**Verification:** benchmark 对比 U1 基线应有显著提升（跳过了最大的目录树）

---

### U4. 集成验证

**Goal:** 跑完整 benchmark 对比 + 实际计时验证 < 30s

**Requirements:** R4

**Dependencies:** U2, U3

**Files:** 无新文件

**Approach:**
- `cargo bench -- scan_purge` 对比 U1 基线
- 实际 `time cargo run --release -- purge ~` 验证 wall-clock < 30s
- 确保 42 个现有测试通过

**Test scenarios:**
- `cargo test` 42 个测试全部通过
- `cargo bench` 显示 improvement

**Verification:** 实测 purge ~ 完成时间 < 30s
