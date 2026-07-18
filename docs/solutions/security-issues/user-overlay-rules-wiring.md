---
title: "用户叠加规则接线：靠策略内既有 pattern filter 分流，删除授权隔离保持不变"
date: 2026-07-18
category: security-issues
module: 规则加载与扫描策略
problem_type: architecture_decision
component: rules-loading
severity: medium
symptoms:
  - "用户在 ~/.config/mc/rules.toml 写的规则加载了却不参与扫描（引擎已实现但 scanner 未接线）"
  - "接入用户规则时担心 Exact/DirName 混合规则集会被错误策略处理，或绕过删除授权隔离"
root_cause: incomplete_wiring
resolution_type: code_fix
related_components:
  - crates/core/src/scanner.rs
  - crates/core/src/rules.rs
---

## 问题

`mc_core::rules` 的用户叠加规则引擎（`user_rules()` 读 `~/.config/mc/rules.toml`、
`user_rules_from_str()` 门禁 + 强制 `preselect=false`、`all_rules()` = 内置 + 用户）
早已实现且有测试，但**扫描器从未接线**：`Scanner::scan_clean` 调 `clean_rules()`、
`scan_purge` 调 `purge_rules()`，都只吃内置规则。`all_rules()` 仅被只读诊断
`evidence_for_path` 采用。用户写的规则静默不生效。

## 解决

两处薄封装各追加 `user_rules()`：

```rust
// scan_clean
let mut rules = clean_rules();
rules.extend(user_rules());
Self::scan_with_rules(&rules, reporter)

// scan_purge
let mut rules = purge_rules();
rules.extend(user_rules());
Self::scan_purge_dir(base_path, &rules, reporter)
```

**为什么不用 pattern 预分流**：两条扫描策略内部本就按 pattern 类型 filter——
`scan_with_rules` 只挑 `PathPattern::Exact`（clean 语义），`scan_purge_dir` 按
`DirName` 剪枝。把完整 `user_rules()`（可含两类 pattern）附加给任一策略是**安全**的：
不匹配该策略的 pattern 被既有 filter 自然忽略，无需在接线层做预分流。更简单、更少出错。

## 两个 load-bearing 不变量

1. **删除授权隔离不变**（安全核心）：`deletion_evidence_for_path` 继续只信
   `builtin_rules()`。用户规则扩大的是**扫描发现的项**（这些项带 `preselect=false`，
   需手动勾选），不能作为任意路径降级为 Safe/Moderate 的依据。这是
   [[analyze-unknown-path-deletion-fail-closed]] 锁定的边界——扫描发现（可含用户规则）
   与删除授权（只信内置）解耦，是本产品安全模型的核心。接线只扩大发现，不打开授权口。

2. **preselect=false 语义链**：`user_rules_from_str` 强制 `preselect=false`，经
   `Meta::from_rule` → `with_preselect` 流到 `ScanItem.selected`。用户规则命中项永不预选，
   `--yes`/默认勾选不删。

## 踩到的认知偏差（proof-first 的价值）

规划时以为「`scan_purge_dir` 只按 DirName」，据此写的隔离测试断言"Exact 用户规则在
purge 里被忽略"。测试**红了**——`scan_purge_dir` 实际同时处理 DirName 剪枝**和** Exact
路径（条件是 `exact_path.starts_with(base_path)`）。失败暴露了误解，据实修正测试为
"base 之外的 Exact 用户规则不命中"（真实且安全的边界：purge 该目录时顺带清其下的
Exact 目标是合理的）。教训：先写断言观察红/绿，比照着臆想的行为写代码更早发现偏差。

## 防漂移

README「用户叠加规则」小节的示例 TOML 由 `readme_example_rules_pass_gate` 契约测试
钉死——示例失效（字段改名、守卫要求变化）即红。
