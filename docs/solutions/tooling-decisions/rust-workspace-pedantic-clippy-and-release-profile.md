---
title: "Rust workspace 开发基础设施：pedantic clippy + workspace lints + release profile"
date: 2026-06-06
category: tooling-decisions
module: development-infrastructure
problem_type: tooling_decision
component: development_workflow
severity: medium
applies_when:
  - 新建 Rust workspace 项目需要统一代码质量标准
  - 已有项目想提升代码质量并启用 pedantic lint
  - 多 crate workspace 需要一致的 lint 配置
  - 需要发布优化二进制（CLI 工具、系统服务）
tags:
  - rust
  - clippy
  - pedantic
  - workspace-lints
  - release-profile
  - toolchain
  - developer-experience
---

# Rust workspace 开发基础设施：pedantic clippy + workspace lints + release profile

## Context

Rust 项目初期往往只用 `cargo clippy` 默认 lint 集，随着代码量增长，潜在的代码质量问题（不必要的 clone、非惯用模式、性能反模式）悄然累积。多 crate workspace 中各 crate 各自配置 lint 导致不一致，且 clippy 每个版本新增的 pedantic lint 不会自动生效，需要手动跟踪。release 构建也缺少统一优化配置，二进制偏大。

本项目从 claude-devtools-rs 项目因地制宜引入了这套开发基础设施配置。

## Guidance

采用 **"全开 pedantic + 渐进式 allow"** 策略，而非传统的"逐条 warn 打开"策略。

### 1. Workspace 级别统一声明 lint 规则

根 `Cargo.toml`：

```toml
[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# 渐进式 allow——只允许当前不适合修复的 lint（附理由）
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
similar_names = "allow"
too_many_lines = "allow"
cast_possible_truncation = "allow"
cast_precision_loss = "allow"
cast_sign_loss = "allow"
items_after_statements = "allow"
unnecessary_wraps = "allow"
match_same_arms = "allow"
struct_excessive_bools = "allow"
```

### 2. 各 crate 继承 workspace lints

```toml
# crates/xxx/Cargo.toml
[lints]
workspace = true
```

### 3. Release profile 统一优化

```toml
[profile.release]
opt-level = 3
lto = "thin"          # fat LTO 编译太慢，thin 平衡速度和优化
codegen-units = 1     # 更好的优化机会
strip = "symbols"     # 减小二进制体积
```

### 4. 固定工具链版本

`rust-toolchain.toml`：

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

### 5. 首次启用后批量修复 + 持续保证合规

```bash
cargo clippy --fix --allow-dirty --allow-staged
```

配合本地 Claude Code hook（编辑 .rs 文件后自动运行 clippy）持续保证合规。

## Why This Matters

**遵循：**
- 新的 clippy pedantic lint 随 Rust 更新**自动生效**，无需手动追踪 clippy changelog
- workspace 级别保证所有 crate lint 配置**一致**
- `unsafe_code = "deny"` 防止意外引入 unsafe 代码
- release profile（strip + LTO）通常可减少 30-50% 二进制体积
- `rust-toolchain.toml` 确保 CI 和本地环境工具链版本一致

**不遵循：**
- lint 配置散落各 crate 导致不一致
- 新增 pedantic lint 永远不会被发现
- release 构建使用默认 16 codegen-units + 无 LTO，性能和体积都不是最优
- 团队成员工具链版本不同导致 CI 结果不可复现

## When to Apply

- 新建 Rust workspace 项目时（第一时间配置，避免后续大量修复）
- 已有项目想提升代码质量时（一次性 `clippy --fix` + 持续 CI 检查）
- 项目有多个 crate 需要统一 lint 标准时
- 需要发布优化二进制（CLI 工具、系统服务）时
- 从另一个 Rust 项目迁移最佳实践时

## Examples

### Before：逐条打开策略

```toml
# 各 crate 各自配置，容易遗漏
# crates/core/Cargo.toml
[lints.clippy]
needless_return = "warn"
redundant_closure = "warn"
# 要手动列出每一条想要的 lint...
```

### After：全开 + 渐进式 allow 策略

```toml
# 根 Cargo.toml —— 一处定义，全局生效
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
# 只 allow 确实不适用的（有明确理由）
module_name_repetitions = "allow"  # 本项目模块名是有意重复的
cast_possible_truncation = "allow"  # 文件大小计算已确保范围
```

### 典型自动修复效果

```rust
// Before: .map(x).unwrap_or(default)
children.iter().find(|(_, _, c)| c == category)
    .map(|(_, s, _)| *s)
    .unwrap_or(root.safety);

// After: .map_or(default, x) —— 更惯用
children.iter().find(|(_, _, c)| c == category)
    .map_or(root.safety, |(_, s, _)| *s);
```

```rust
// Before: 手动 if let 展开
for entry in children.iter_mut() {
    if let Ok(dir_entry) = entry {
        // ...
    }
}

// After: Iterator::flatten 自动跳过 Err
for dir_entry in children.iter_mut().flatten() {
    // ...
}
```

## Related

- 源自 claude-devtools-rs 项目的开发基础设施实践
- Clippy pedantic lint 列表: https://rust-lang.github.io/rust-clippy/master/index.html
