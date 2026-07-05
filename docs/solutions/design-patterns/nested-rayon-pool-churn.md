---
title: "嵌套 rayon 线程池 churn：并行 map 里每项再建新池 = CPU 空耗"
date: 2026-07-05
category: design-patterns
module: mc-core
problem_type: performance
component: background_job
severity: high
applies_when:
  - "在 rayon par_iter / 线程池的 map 里，对每个元素又构造一个会新建线程池的调用"
  - "用 jwalk 且 Parallelism 设为 RayonNewPool（而非 Serial / RayonExistingPool）"
  - "CPU 占用远超核数、sys（内核态）时间畸高，但墙钟并不更快"
  - "并行处理许多中小任务，单任务内部又想并行"
tags:
  - rust
  - rayon
  - jwalk
  - thread-pool
  - cpu-usage
  - nested-parallelism
  - scanner
  - performance
---

# 嵌套 rayon 线程池 churn：并行 map 里每项再建新池 = CPU 空耗

## Context

macCleaner 的 Purge 扫描要算许多匹配目录（node_modules / target / …）的总大小。管线是：
剪枝遍历收集匹配目录 → 在一个 4 线程池（`build_dir_size_pool`，`crates/core/src/scanner.rs`）里
`par_iter` 并行对每个目录调 `dir_size()`。而 `dir_size()` 内部用 `create_walker()` 遍历该目录，
walker 的并行度设为 `jwalk::Parallelism::RayonNewPool(3)`（macOS）。

用户反馈：TUI 扫描期活动监视器 CPU **稳定 150-200%**，几个命令都这样，扫完才降。

## Problem

`jwalk` 0.8.1 的 `RayonNewPool(n)` 语义是**每次把 walker 转成迭代器就 `ThreadPoolBuilder::new().build()`
构造一个全新的 n 线程池、用完即销毁**（源码 `lib.rs:525-535` + `read_dir_iter.rs:63`，无缓存/不复用）。

于是外层 4 线程池并行处理 M 个目录时，**每处理一个目录就新建一个 3 线程池**：

```
dir_size_pool(4).install(|| dirs.par_iter().map(|d| dir_size(d)))
                                              └─ dir_size -> create_walker -> RayonNewPool(3)  // 每个 d 一个新池
```

后果：
- 峰值并发 walker 线程 ≈ 4（外层）+ 4×3（同时在算的 4 个目录各带一个内层池）= **~16 线程**。
- 整轮扫描创建/销毁约 **M 次** 3 线程池。
- CPU 大量烧在**线程创建 + 上下文切换**（内核态 sys 时间），却不产出扫描进度。

实测（受控 A/B，`~/workspace` dry-run，release，暖缓存交替 3 轮）：

| | 墙钟 | user+sys = CPU 秒 | avg CPU% | 其中 sys |
|---|---|---|---|---|
| 嵌套池（RayonNewPool per dir） | 6.53s | ~20.4 | ~312% | ~15.4s |
| 串行 dir_size（并行由外层提供） | 6.65s | ~7.2 | ~108% | ~5.8s |

即：**CPU 降 ~2.8×（−65%），墙钟 +1.8%（噪声内不退化）**。sys 从 ~15s 掉到 ~6s——空耗被砍掉，真正的遍历工作量没变，所以速度守住。

## Solution

**当外层已有并行（par_iter over 元素），内层单任务改串行遍历**——把 `dir_size` 的 walker 从
`RayonNewPool(n)` 换成 `jwalk::Parallelism::Serial`（`create_walker_serial`）。并行度完全由外层
`dir_size_pool` 的 `par_iter` 提供，总线程数收敛到外层池大小（默认 4），不再有内层池、不再有 churn。

为什么选 `Serial` 而不是 `RayonExistingPool` 复用同一个池：若内层 walker 复用**外层 par_iter 正在跑的那个池**，
会出现"外层 worker 阻塞等 walk 结果，而 walk 又需要同池 worker 去产出"的**同池嵌套消费 self-lock 风险**。
`Serial` 在调用线程上同步遍历、不触碰任何 rayon 池，从根上规避。

关键：**改动不改变结果语义**——`Serial` 仍触发 `process_read_dir(prefetch_metadata)` 回调填充文件大小，
大小求和、每 1024 entry 取消检查、不跟随符号链接均不变（仅遍历顺序变，与求和无关）。回归测试
`test_scan_purge_many_dirs_parallel_no_deadlock`（12 目录 > 4 线程）断言无死锁 + 大小正确。

## When NOT to apply / 边界

- **单个超大目录**（如 20GB 级 target/DerivedData）串行遍历理论上比内层 3 线程慢。实测被众多小目录的
  外层并行摊平、墙钟未退化。若未来出现"单个超大目录扫描慢"，正解是给 `dir_size` 接一个**共享持久
  walk 池**（`RayonExistingPool` 传入**独立于** par_iter 的池），恢复大目录内层并行且不重蹈每目录建池——
  而不是回退到 `RayonNewPool`。
- 若外层**没有**并行（单任务场景），内层用池并行是对的——本坑特指"外层已并行、内层再建池"的嵌套。

## 续篇：U6 消了 churn，但留下"过度订阅自旋"（2026-07-05 后续探测）

对 `purge ~`（全盘，非 `~/workspace`）全程 `sample` 发现：U6 后仍有 **~25% on-CPU 时间在
`sched_yield`（`swtch_pri`）自旋**，最大一簇 31869 样本在**主线程** `cthread_yield→swtch_pri`。根因是
**过度订阅**，不是 churn：

- 默认 `WALK_THREADS=0` → 剪枝遍历走 `RayonDefaultPool` = **rayon 全局池 = num_cpus 线程**（本机 10 核 = 10 线程），
  加 `dir_size_pool` 4 线程 + 主线程 steal-spin。macOS I/O-bound 扫描下，idle worker 自旋等活。
- U6 把 `dir_size` 改串行消了建池 churn，但把成本转成了 **straggler 自旋**：一个巨型 node_modules 时，
  外层池其余 worker 无活可偷 → `sched_yield` 空转（早期采样见 4 线程 98% 在 swtch_pri）。

**对标 dua-cli**（本项目并发基准，`/Users/zhaohejie/workspace/explore/dua-cli`）：
`options.rs` 明确写 *"on macOS too many threads are bad, 3 is best on M4"* 默认 3 线程；
`main.rs` 注释 *"avoid using the global rayon pool, it will keep a lot of threads alive"* → threads=0 时
**刻意避开全局池**，改用 3 线程持久 `RayonExistingPool { busy_timeout: None }`。**我们默认恰好踩了它避开的坑。**

**未验证的候选修法**（需受控 A/B，勿盲改）：用单个 3–4 线程**持久**池（对齐 dua），剪枝遍历不走 10 线程全局池。
注意：**朴素减线程数已被实测证伪**——`MC_DIRSIZE_THREADS=2` 给 232%/97s（比默认 225%/86s 更差）、
`MC_WALK_THREADS=3` 给 218%/98s（CPU 微降墙钟更长），因为最大自旋在主线程等池，减 worker 只让池更慢、主线程等更久。
正解是"小而持久的池 + 合适的 parking"，不是简单调小数字。衡量收益要看 **profiler 的 swtch_pri 样本占比**，
不要看墙钟（被 EDR 的 close 噪声淹没，见 [[edr-syscall-tax-distorts-cpu-measurement]]）。

## Takeaways

- **诊断信号**：CPU% 远超核数 + sys 时间畸高 + 墙钟并不更快 = 强烈提示线程过度订阅 / 池 churn，而非有效并行。
- **churn ≠ 过度订阅**：消了"每目录建池"（churn）不等于消了"太多线程空转"（过度订阅）；两者都表现为高 sys，
  但前者靠"内层串行/复用"修，后者靠"限制池大小 + 持久化"修。
- **测量前提**：CPU/并发结论只在 **release** 下可信（LTO + opt-level=3）；用**合成固定目录树**（tempfile）而非真实 home 做基准，才能跨配置复现；`MC_WALK_THREADS`/`MC_DIRSIZE_THREADS` env 旋钮支持线程数扫参。
- **并行只加一层**：嵌套并行要么共享一个池（且避免同池自锁），要么内层串行让外层负责并行；切忌"每个元素新建一个池"。
- 相关：`docs/plans/2026-07-05-009-perf-scan-cpu-optimization-plan.md`（本次计划与完整测量）、`docs/solutions/tooling-decisions/rust-workspace-pedantic-clippy-and-release-profile.md`（release 测量前提）。
