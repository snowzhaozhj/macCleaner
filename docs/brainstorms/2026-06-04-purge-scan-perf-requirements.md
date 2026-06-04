---
id: brainstorm-005
title: Purge 扫描性能 benchmark + 优化
date: 2026-06-04
status: accepted
---

# Purge 扫描性能 benchmark + 优化

## 概述

STRATEGY.md 目标「全盘扫描 < 30s」当前未达标（purge 扫 ~ 超过 30s）。通过建立 criterion benchmark 基线、实施两个架构级优化（Exact 规则直接 stat + 扩展跳过目录列表），将扫描时间降至目标以内。

## 核心需求

### R1: Criterion benchmark 基线

- 为 `scan_purge_dir()` 建立可复现的 benchmark
- 使用 criterion crate，benchmark 覆盖典型扫描路径（如 workspace 目录）
- 后续 PR 可通过 `cargo bench` 检测性能回归

### R2: Exact 规则从遍历中剥离

- 10 条 Exact 规则（Docker、Maven、Homebrew、Go、Cargo、npm/pnpm/yarn、pip、Xcode Archives、Android、JetBrains）当前被混在目录遍历中
- 改为直接 stat 路径存在性 + 并行计算目录大小
- 不再让这些路径影响 jwalk 遍历的搜索范围

### R3: 扩展跳过目录列表

- 当前 SKIP_DIRS 只有 `.git`、`.Spotlight-V100`、`.fseventsd`
- purge 扫描应额外跳过不可能包含开发产物的大目录：`Library`、`Applications`、`.Trash`、`Pictures`、`Music`、`Movies`
- 仅对 purge 模式生效，不影响 clean 和 analyze

### R4: 优化后 benchmark 验证

- 优化实施后重跑 benchmark，对比基线
- 实际计时 `mc purge ~` 确认 < 30s（在用户机器上）

## 成功标准

- `cargo bench` 能运行并输出 scan_purge 耗时
- 优化后 purge 扫描 `~` < 30s
- 现有 42 个测试不回归
- 扫描结果正确性不变（Exact 规则仍能发现对应目录并计算正确大小）

## 非需求

- 不做增量缓存（后续独立 feature）
- 不改 jwalk 线程数（当前 3 线程是 macOS APFS 的合理配置）
- 不优化 clean 命令（它用 Exact 规则遍历已有路径，性能不是问题）
- 不做 GUI/UX 层面的感知优化（如进度条动画）
