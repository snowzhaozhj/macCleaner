---
title: "反向卸载孤儿残留的三道误杀防线"
date: 2026-07-19
category: security-issues
module: 应用残留清理链路
problem_type: security_issue
component: authentication
severity: high
symptoms:
  - "反向扫描 ~/Library 时，系统组件残留（com.apple.*）被误判为父应用已卸载的孤儿"
  - "刚删除应用的残留立刻被列为可删，用户马上重装时数据已进废纸篓"
  - "普通目录名（非 bundle-id 反向域名）被当作孤儿候选，误杀多产品共用容器"
root_cause: logic_error
resolution_type: code_fix
related_components:
  - "mc-core app_resolver"
  - "mc-core engine"
  - "mc CLI orphans"
tags:
  - "uninstall"
  - "orphan-scan"
  - "reverse-uninstall"
  - "fail-closed"
  - "false-positive"
  - "path-safety"
  - "bundle-id"
---

# 反向卸载孤儿残留的三道误杀防线

> `component: authentication` 沿用本 schema 中最接近“破坏性动作授权闸门”的取值；实际实现落在 `mc-core` 扫描侧与 CLI。

## Problem

正向卸载（`find_leftovers(bundle_id)`）从一个**仍安装**的应用出发找它的残留——父应用在，匹配范围收敛，误杀风险低。反向卸载（孤儿扫描）方向相反：枚举 `~/Library` 里的残留，反查**父应用是否还在**。这个方向天生危险：

1. **系统组件伪装成孤儿**：`~/Library` 常年存在 `com.apple.*` 等系统组件的缓存/偏好，它们的"父 App"是系统而非可卸载应用——按"父 App 不在已装列表"的朴素判据会把它们全列为孤儿。
2. **刚删残留 ≠ 该删**：用户可能刚把应用拖进废纸篓、马上要重装恢复配置；此刻残留虽"无主"，删掉反而毁掉用户想保留的状态。
3. **普通目录名不是 bundle-id**：`Caches/Google/`、`Caches/Microsoft/` 这类多产品共用容器目录名不是反向域名 bundle-id，无法可靠反查父应用；当孤儿删会误杀仍在用的产品数据。

误杀不可逆代价远高于漏报——孤儿是"回收"非"必删"，漏了可再扫。

## Resolution

`AppResolver::scan_orphans`（`crates/core/src/app_resolver.rs`）用**三道串联防线**把误报压到最低，全部偏向 fail-closed：

1. **fail-closed bundle-id 析取**（`extract_bundle_id`）：只有条目名含 ≥2 个 `.`（形如 `com.vendor.App` 的反向域名）才认作 bundle-id 候选；普通目录名、单段名析不出 → 直接跳过、不当孤儿。宁可漏掉一个真孤儿，也不误判一个共用容器。
2. **系统预留黑名单**（`RESERVED_BUNDLE_PREFIXES`）：候选前缀命中 `com.apple.` 等系统/共享前缀 → 排除。首版只硬保 `com.apple.`；其余共享前缀（`com.google.` 下多产品共用等）按真机误报反馈迭代追加，不首版穷举（穷举易漏、且无真机数据支撑）。
3. **龄阈值**（`ORPHAN_MIN_AGE_DAYS`，默认 30 天）：残留目录 mtime 距今不足阈值 → 跳过，给"刚删可能重装"缓冲期。读不到 mtime 时保守视为"太新"跳过（也是 fail-closed）。

父应用存在性判定（`bundle_installed`）是**正向匹配规则的补集**，与 `find_leftovers` 的匹配语义对称：相等、或候选是某已装 id 的 `id.`/`id-` 派生（残留带 hash/后缀）、或某已装 id 是候选的同形派生。双向前缀关系确保带 `.savedState` 等后缀的残留能归位到仍安装的父应用、不误列孤儿。

**分级比正向更保守**：残留分级沿用 issue #25 rubric（`USER_DATA_SUBDIRS` → `Moderate` + 证据文案，其余 → `Safe`），但孤儿场景把 `preselect` **统一关掉（含 Safe 项）**。理由：正向卸载里用户已明确选中要卸的应用，残留跟着删是预期；孤儿是工具主动发现的，用户没表达删除意图，且应用已卸载但数据可能是有意保留的。CLI 层因此让 `mc orphans --yes` **不自动删任何东西**（无预选项），并明确提示须交互指定编号——避免"静默无操作"的困惑。

删除授权侧不受影响：本功能只扩大**发现范围**，`deletion_evidence_for_path` 仍只信内置规则（见 [Analyze 未知路径 fail-closed]）。

## Key Takeaway

反向发现（枚举产物反查所有者）比正向发现（从所有者找产物）误杀面大一个量级——匹配范围从"一个已知 id"放大到"整个目录树"。防线要**多道串联且全部偏保守**：无法可靠反查所有者的候选（析不出身份、命中系统预留、龄不明）一律排除，宁漏报不误杀。分级之外再独立收紧 `preselect`，让"工具主动发现"的项永不进入自动删除路径。

## Related

- `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`（同源 fail-closed / 只信内置规则原则）
- `docs/plans/2026-07-19-027-feat-orphan-leftover-scan-plan.md`（本功能实现计划）
- roadmap issue #27 方向 #5；前置安全债 issue #25（app_resolver 残留分级复议）已合并 PR #29
