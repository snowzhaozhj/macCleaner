---
name: worktree-parallel-dev-foundation
title: worktree 并行开发基线 - Plan
date: 2026-07-06
type: chore
topic: worktree-parallel-dev-foundation
artifact_contract: ce-unified-plan/v1
artifact_readiness: requirements-only
product_contract_source: ce-brainstorm
execution: code
---

# worktree 并行开发基线 - Plan

## Goal Capsule

- **目标**：让 git worktree 成为一等的并行开发环境——共享的技能、工作流 hook、忽略规则都随仓库走，worktree 内开发质量与主仓一致，且开箱即用、零 bootstrap。
- **产品裁决权**：用户（仓主）。本文所有产品决策已在 brainstorm 敲定。
- **未决阻塞**：无。唯一需实证的点（worktree 内 hook 是否真触发）在 R5 以"实测后据实落文档"的方式吸收，不阻塞落地。

## Product Contract

### Summary

把"共享开发配置随仓库走"从半吊子状态收敛成统一模型：提交本地技能实体 + 符号链接、删掉从未物化的 `skills-lock.json`、把 impeccable 缓存的忽略规则从本地私有 exclude 移到共享 `.gitignore`，并实测校正 `CLAUDE.md` 里关于 worktree 内 hook 生效性的过时论断。产出一次 PR，交付后 worktree 里 clone 出来即拥有与主仓同等的质量门。

### Key Decisions

- **技能物化模型统一为"纯本地实体 + 符号链接"。** `.agents/skills/<name>/`（真实内容）为唯一真相源，`.claude/skills/<name>` 为指向它的相对符号链接；两者都提交，符号链接在各自工作树内自解析。放弃"lock 文件 + github fetch"这条从未真正跑通的路径。
- **删除 `skills-lock.json` 而非清空。** 它只锁了一个从未物化、全仓无消费者、"感觉没用上"的 `ast-grep` 技能。留空壳锁文件与新模型不搭，直接删。
- **worktree 质量门靠"提交配置文件"实现，不引入新机制。** `.claude/settings.json` 与 hook 脚本已跟踪、天然随 worktree 存在；`deny-edit-on-main.sh` 已 worktree-aware（读 gitdir 的 HEAD）。基线不新增编排，只补齐"技能随行"这块缺口并据实修文档。

### Requirements

**技能随仓库走**

- R1. 提交 `.agents/skills/verify-tui/SKILL.md`（本地技能实体）与 `.claude/skills/verify-tui` 符号链接，使技能随 worktree/clone 自动到位。
- R2. 保留 `.gitignore` 中对 `.agents/` 与 `.claude/skills/` 的 un-ignore（本次 brainstorm 起点），不回退。
- R3. 删除 `skills-lock.json`。删后全仓不得再有引用它的路径。

**worktree 质量门**

- R4. 确认 `.claude/settings.json` 及两个 hook 脚本在 worktree 检出中存在且可执行——这是质量门随行的机械前提。
- R5. 在真实 Claude 会话的 worktree 内实测 `deny-edit-on-main` 与 `clippy-after-rs-edit` 是否触发，并据结果修正 `CLAUDE.md:92`：若触发，改掉"在 worktree 内该 hook 不生效"这句过时论断；若确不触发，保留告警并写明补偿动作（手动 `cargo clippy`）。结论必须来自实证，不得靠推断。

**忽略规则卫生**

- R6. 把 `.impeccable/hook.cache.json` 的忽略规则从 `.git/info/exclude`（本地私有、新 clone 不继承）移到 `.gitignore`（共享），防止新 clone / worktree 误提交该生成缓存。

**文档**

- R7. 在 `CLAUDE.md` 补一节 worktree 并行开发约定：技能随行的模型、hook 质量门在 worktree 的状态（R5 结论）、起/清 worktree 的方式（`ce-worktree` 起、既有 `scripts/clean-worktrees.sh` 清）。

### Acceptance Examples

- AE1. **覆盖 R1、R3。** 在本次改动提交后新起一个 worktree：`.claude/skills/verify-tui/SKILL.md` 经符号链接可读到内容；仓库根不存在 `skills-lock.json`。
- AE2. **覆盖 R5。** 在 worktree 内的 `main`（或 detached HEAD）尝试编辑 `.rs` 源码：若 hook 生效则被 `deny-edit-on-main` 拦截——以此实证 hook 是否随行，据此定 `CLAUDE.md` 措辞。
- AE3. **覆盖 R6。** 在新 clone（不含 `.git/info/exclude` 定制）中，`.impeccable/hook.cache.json` 处于被忽略状态，不出现在 `git status`。

### Scope Boundaries

- 不新建 worktree 创建脚本——`ce-worktree` 已覆盖创建，`scripts/clean-worktrees.sh` / `clean-all.sh` 已覆盖清理。
- 不改 hook 脚本逻辑（除非 R5 实证暴露真实缺陷）。
- 不引入跨平台符号链接兼容层——本项目是 Mac 工具，`core.symlinks` 默认开，Windows 兼容不在本次范围。
- 不改 `.claude/settings.local.json` 的个人/本地语义（保持 gitignore）。

### Assumptions

- `.claude/skills/verify-tui` 的符号链接目标为仓库内相对路径 `../../.agents/skills/verify-tui`，提交后在任意工作树内自解析（已实测：探针 worktree 中缺失仅因改动尚未提交）。
- 无外部 skill 管理 CLI 会在删除后自动重建 `skills-lock.json`；若重建也仅为无害空壳。
