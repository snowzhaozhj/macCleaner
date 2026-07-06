---
title: "企业 EDR 给文件 syscall 加税：扫描 CPU 测量会被污染，先建干净基线"
date: 2026-07-05
category: tooling-decisions
module: mc-core
problem_type: performance
component: measurement
severity: high
applies_when:
  - "在装了企业 EDR / Endpoint Security 客户端的 Mac 上测扫描/遍历类 CPU"
  - "profiler 里 open/close 占比远高于 getdirentries/lstat（开关句柄比读内容还贵）"
  - "sys（内核态）时间畸高，且换机器/换环境后数字大幅变化"
  - "要判断某个 CPU 优化的收益是否真实、是否发生在真实用户身上"
tags:
  - macos
  - endpoint-security
  - edr
  - cpu-usage
  - measurement
  - syscall
  - scanner
---

# 企业 EDR 给文件 syscall 加税：扫描 CPU 测量会被污染，先建干净基线

## Context

plan 009（`docs/plans/2026-07-05-009-perf-scan-cpu-optimization-plan.md`）在本开发机上测扫描期 CPU，
得出「U6 消除嵌套池后 CPU 降到 ~1 核（108%）、已到收益拐点」的结论。后续深入探测发现：
**该结论测小了，也被本机环境污染。**

在**全盘** `purge ~`（CLI 与 TUI 引擎同源）上实测：CPU **~220%（2.3 核）**，多轮 219–232%——
**复现了用户最初「150-200%」的抱怨**。plan 的 108% 只是因为它测的是 `~/workspace` 这棵小树；
CPU 随扫描树规模放大，不是固定 1 核。

## Problem

对 `purge ~` 全程 `sample`（1ms tick，时间加权叶子帧），on-CPU 时间构成：

| 去向 | 占 on-CPU | 帧数（`Sort by top of stack`） |
|---|---|---|
| `close` / `closedir` | **47%** | 180465 |
| `open` | 13% | 35718 |
| `getdirentries` + `lstat` + `stat` | 17% | 32235 + 26022 + 7260 |
| `sched_yield`（`swtch_pri`）自旋 | 25% | 91164 |

**决定性观察**：`open`+`close`（开关目录句柄）占 **57%**，而真正读数据的 `getdirentries`+`lstat` 只占 17%。
开关句柄比读内容贵得多——正常机器上 `close()` 近乎免费，这里 `close` 是 `open` 的 **5 倍**样本。
这是**每次 syscall 有固定内核开销**的铁证，不是算法问题。

根因：本机运行**阿里企业 EDR**——`com.alibaba.endpoint.aliedr.ne` 系统扩展（macOS Endpoint Security
客户端）+ `/opt/oneagent/edr/` 全套安全 agent（`systemextensionsctl list`、`ps -Ao comm | rg aliedr/oneagent`
可查）。macOS ES 框架会在 `open`/`close`/`exec` 等文件事件上挂**授权/通知回调**，每次 `close` 都要
唤醒 EDR agent 处理一次——这就是 close 畸贵的来源。

**遍历磁盘 = 海量 open/close 目录句柄 = 海量 EDR 回调**，于是扫描 CPU 被系统性放大。

## Solution / 决策

1. **不要用被 EDR 污染的数字下"固有成本"结论。** plan 009 曾把高 sys 归为"fs syscall 固有开销、
   到达收益拐点"——错。约一半 on-CPU（open+close ~57%）是 EDR 的 per-syscall 税，**真实用户的非企业
   Mac 上大概率不出现**。

2. **优化前先建干净基线**：在无 EDR 的 Mac 上复测同一扫描。若 CPU 掉到 ~1 核，则说明剩余是环境因素，
   投资应结束；若仍高，再优化。**没有干净基线时，可能在优化一个真实用户看不到的数字。**

3. **无干净机时，改用"环境不变量"做优化目标**：
   - **忽略** EDR 税那部分（不可控），聚焦**任何机器上都是浪费**的项——本例是 25% 的 `sched_yield`
     自旋（jwalk 并行迭代器**消费端固定忙等**，与池大小无关；后由 **park 式阻塞遍历器**消除——空闲挂起
     而非自旋，见 [[nested-rayon-pool-churn]] 续篇与 issue #20。注：曾尝试"改默认 Serial"消自旋，但 Serial
     让 analyze 慢 76%，已弃用）。
   - **用 profiler 的 `swtch_pri` 样本占比**衡量自旋优化效果，而非墙钟/CPU% 秒数——后者被 EDR 的
     close 噪声淹没（单次 `purge ~` 墙钟方差 85–98s，比要测的效应还大）。
   - 减少 syscall **总数**的优化（如批量枚举 `getattrlistbulk`）在干净机和本机上**双赢**（既省真实
     开销，又省 EDR 回调），因为 EDR 是按 syscall 计费的。

## 诊断信号（怎么识别这个坑）

- profiler 里 `open`/`close` 占比 > `getdirentries`/`lstat`（开关句柄比读内容贵）。
- `close` 样本远多于 `open`（正常应 ≈1:1）。
- sys 时间畸高，换机器后数字大幅变化。
- `systemextensionsctl list` 有第三方 ES / `com.apple.system_extension.*`；`/opt/oneagent/`、
  `crowdstrike`/`falcon`/`sentinel`/`jamf`/`aliedr` 等进程在跑。

## Takeaways

- **企业 Mac 上的 I/O 密集 CPU 测量默认不可信**——先排查 EDR，再下结论。
- **区分"可控浪费"与"环境税"**：优化前把 profiler 拆成 { 自旋/锁等浪费 | 真实工作 | 环境税 }，
  只对前两类动手。
- 相关：[[nested-rayon-pool-churn]]（同一次扫描的消费端自旋续篇，解法为 park 式阻塞遍历器）、
  `docs/plans/2026-07-05-009-perf-scan-cpu-optimization-plan.md`（被本文修正的原始结论）、
  `docs/solutions/tooling-decisions/rust-workspace-pedantic-clippy-and-release-profile.md`（release 测量前提）。
