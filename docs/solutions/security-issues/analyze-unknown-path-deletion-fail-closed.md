---
title: "Analyze 未知路径删除必须 fail-closed"
date: 2026-07-11
category: security-issues
module: Analyze 删除安全链路
problem_type: security_issue
component: authentication
severity: high
symptoms:
  - "Analyze 中未命中内置清理规则的任意路径会被当作 Safe，确认框缺少真实风险证据"
  - "用户文档或有价值的应用状态可能绕过 Risky 的 delete 强确认而进入删除流程"
  - "确认口令未绑定到确认框当时展示为 Risky 的具体路径，执行前风险变化缺少重新确认闸门"
root_cause: logic_error
resolution_type: code_fix
related_components:
  - "mc-core rules"
  - "mc-gui Tauri IPC"
  - "mc-tui analyzer deletion"
tags:
  - "analyze"
  - "fail-closed"
  - "path-safety"
  - "destructive-action"
  - "type-to-confirm"
  - "authorization"
  - "rule-evidence"
  - "toctou"
---

# Analyze 未知路径删除必须 fail-closed

> `component: authentication` 是当前文档 schema 中最接近“破坏性动作授权闸门”的取值；实际实现横跨 `mc-core`、Tauri GUI 与 TUI。

## Problem

Analyze 与 Clean/Purge 的信任边界不同：它先把任意目录做空间导航，用户随后可以标记其中任意文件或目录，并不是先由清理规则筛出候选项（`CONCEPTS.md:9`、`CONCEPTS.md:11`）。因此，“没有命中规则”只代表没有可用于证明安全性的证据，绝不能推导为 Safe。若删除入口把 `evidence_for_path(path) == None` 当作 Safe，用户文档、应用状态或其他不可再生数据就能绕过 Risky 的 type-to-confirm，直接进入废纸篓删除流程。

这个边界还包含时间维度：`DirName` 规则依赖路径类型和项目 marker，确认框展示后、真正开始删除前，这些条件可能变化。一次笼统的“本批次已经输入过口令”不足以授权变化后的集合，因为它没有记录用户实际看见并确认的是哪些 Risky 路径。

修复已提交到 [PR #45](https://github.com/snowzhaozhj/macCleaner/pull/45)，文档撰写时 PR 仍为 OPEN。

## Symptoms

- Analyze 中未命中任何规则的普通路径曾可按 Safe 处理，确认框不要求输入 `delete`；这与任意路径入口的威胁模型相冲突。核心回归测试现在要求未知路径得到 Risky 和非空影响/恢复证据（`crates/core/src/rules.rs:520`）。
- 同名文件或符号链接可能表面上符合 `DirName`，但并不等于规则所描述的可重建目录；普通文件和带有效 marker 的符号链接都不得降低风险（`crates/core/src/rules.rs:565`、`crates/core/src/rules.rs:586`）。
- live Analyze 的确认与删除之间会跨过后台 `finalize`。若 marker 在这段时间消失，路径可能从 Moderate 升为 Risky；测试要求此时停留在 Analyze、重新展示强确认，而不是进入 Cleaning（`crates/tui/src/lib.rs:995`）。
- 仅保存一个批次级布尔值会让已经确认的 Risky 项替后来升级的另一条路径“借用”授权。逐路径测试要求新 Risky 路径不在原授权集合时必须拒绝（`crates/tui/src/delete.rs:276`、`crates/gui/src/commands/analyze.rs:288`）。

## What Didn't Work

1. **`None => Safe` 的本地 fallback。** 这把“证据缺失”误写成“已证明安全”，在 GUI 与 TUI 各自推断时尤其容易漂移。只读诊断入口仍可返回 `None`，但契约必须明确禁止删除调用方据此授权（`crates/core/src/rules.rs:236`）。
2. **接受第一条匹配规则。** 路径可能同时命中宽泛 Exact 与更具体的规则；若按规则顺序取第一条，低风险规则可能遮住高风险证据。风险选择必须与声明顺序无关，最高风险优先，同风险时再取更具体路径（`crates/core/src/rules.rs:278`）。
3. **只按 basename 匹配 `DirName`。** 名为 `node_modules` 的普通文件、指向任意位置的符号链接，或缺少 `package.json` 的同名目录，都不能证明它是规则描述的开发产物。仅凭名字降低风险会制造错误证据。
4. **批次级 `risky_confirmed: bool`。** 布尔值只能说明用户输入过口令，不能证明某个当前为 Risky 的路径曾以 Risky 状态出现在确认框。异步 finalize 或 marker 变化后，它会过度授权整个批次。
5. **只在打开确认框时分类。** 展示时的分类只适合渲染，不足以成为执行授权；GUI 请求可被直接构造，TUI 也可能在确认后等待后台排序。必须在执行侧重新分类并校验授权。

## Solution

核心层把“解释规则命中”和“授权删除”拆成两个契约：

- `evidence_for_path` 保留为只读诊断入口，可读取内置与用户叠加规则，并在无证据时返回 `None`；删除场景统一使用 `deletion_evidence_for_path(s)`。后者只加载随二进制审计、测试过的内置规则，批量版本只解析一次并保持输入顺序；未知路径统一返回 Risky，以及明确的数据丢失影响和废纸篓恢复边界（`crates/core/src/rules.rs:229`、`crates/core/src/rules.rs:247`、`crates/core/src/rules.rs:256`）。用户规则可以扩展扫描，但不能把 Analyze 的任意路径降为 Safe/Moderate。
- 多规则命中时按 `(风险等级, 具体度)` 选证据：Risky 高于 Moderate/Safe，只有风险相同才选择更长的 Exact 前缀或更具体的目标路径（`crates/core/src/rules.rs:282`、`crates/core/src/rules.rs:303`）。对应测试反转规则顺序，保证结果不依赖声明顺序（`crates/core/src/rules.rs:609`、`crates/core/src/rules.rs:646`）。
- `DirName` 在名字匹配之外，用 `symlink_metadata` 检查路径本身是未跟随符号链接看到的真实目录，再校验 `root_markers`；任何元数据读取失败、符号链接、普通文件或 marker 缺失都不产生降低风险的规则证据（`crates/core/src/rules.rs:311`）。

GUI 将核心证据贯穿展示和执行：`classify_marked` 返回 safety、impact、recovery；前端在查询失败或漏回单条路径时，本地也只会降级为 Risky（`crates/gui/src/commands/analyze.rs:69`、`crates/gui/frontend/src/routes/Analyze.svelte:144`）。用户提交时，前端同时发送口令与确认框中实际显示为 Risky 的路径集合（`crates/gui/frontend/src/routes/Analyze.svelte:196`）。后端从保存的 Analyze 树重新收集路径，释放树锁后重新调用核心分类，并要求当前所有 Risky 路径都出现在 `confirmed_risky_paths` 中，最后才调用废纸篓删除（`crates/gui/src/commands/analyze.rs:99`、`crates/gui/src/commands/analyze.rs:181`）。

TUI 使用同一个批量核心入口构造确认项（`crates/tui/src/delete.rs:28`）。输入正确 token 后，不再只保存布尔值，而是从当次确认清单提取 `HashSet<PathBuf>`；live Analyze 跨 finalize 时，把待删项与该授权集合封装在同一个 `PendingAnalyzerDelete` 中，避免两份并行状态错配（`crates/tui/src/delete.rs:75`、`crates/tui/src/app.rs:616`）。稳定树消费暂存请求后，`start_cleaning_from_analyzer` 在启动废纸篓线程前再次分类；若出现未获授权的新 Risky 路径，就重新打开确认框并展示刷新后的证据（`crates/tui/src/lib.rs:103`、`crates/tui/src/delete.rs:194`）。

## Why This Works

这套设计让降低风险成为“需要正证据”的动作：只有内置规则、完整的模式约束和真实文件系统条件同时成立，路径才可能被判为 Safe/Moderate；所有缺失、失败和未知情况都自然落入 Risky。最高风险优先避免重叠规则的低风险证据覆盖危险证据，同级具体度则保留最贴近目标路径的影响与恢复说明。

逐路径授权集合把用户意图绑定到确认时真正看见的对象，而不是绑定到一次按键或整个批次。GUI 后端闸阻止绕过前端直接调用 IPC，TUI 的 finalize 前后复核覆盖其异步状态转换；后来升级的路径没有集合成员资格，必须重新展示证据并再次输入 token。关键契约由未知路径、DirName marker/文件类型/符号链接、规则优先级、批量一致性、GUI 路径授权、TUI token 绕过与 finalize 期间 marker 变化测试共同固定（`crates/core/src/rules.rs:520`、`crates/core/src/rules.rs:586`、`crates/core/src/rules.rs:678`、`crates/gui/src/commands/analyze.rs:288`、`crates/tui/src/delete.rs:276`、`crates/tui/src/lib.rs:995`）。前端 E2E 还验证未知路径和漏回分类都保持强确认、展示非空证据（`crates/gui/frontend/e2e/analyze.spec.ts:114`、`crates/gui/frontend/e2e/analyze.spec.ts:135`）。

边界必须明确：最终复核与操作系统废纸篓移动并不是原子事务。分类完成到实际处理路径之间仍存在一个窄 TOCTOU 窗口（GUI 见 `crates/gui/src/commands/analyze.rs:205`，TUI 见 `crates/tui/src/delete.rs:203`）。当前措施降低了跨 UI、IPC 和长时间 finalize 的主要风险，但不能宣称消除了所有文件系统竞态；若未来威胁模型要求抵抗同机主动攻击者，需要基于目录句柄或文件标识的删除协议，而不是继续增加布尔检查。

## Prevention

- 任何从 Analyze、文件选择器、拖放或外部参数接受任意路径的删除入口，都必须调用 `deletion_evidence_for_path(s)`；禁止把 `evidence_for_path == None`、IPC 失败或漏回结果设为 Safe。
- 新增或修改规则时，继续维护“最高风险优先、同级最具体”的顺序无关测试；`DirName` 规则必须验证真实目录、拒绝符号链接，并满足 `root_markers` 后才能降低风险。
- 强确认状态必须保存“当时展示为 Risky 的路径集合”，不能退化成批次布尔值。任何异步边界或可变 marker 之后，都要在执行侧重新分类，并对新增 Risky 路径重新展示证据。
- 前端分类只负责用户可见证据和交互升级，后端/TUI 执行入口必须保留独立闸门；实际删除继续限定为废纸篓模式（`CONCEPTS.md:36`）。
- 评审删除安全代码时，显式检查最后一次分类到系统删除之间的 TOCTOU 边界。不得把“执行前复核”描述为与删除原子化；若边界扩大或攻击模型改变，应升级底层删除原语。

## Related Issues

- [流式聚合的键必须是动作粒度（路径），而非展示粒度（分类）](../design-patterns/streaming-aggregation-key-is-action-granularity.md)：`PathBuf` 是删除动作身份；本学习进一步要求授权也绑定到具体路径。
- [增量构建树的实时排序：显示层索引置换，而非原地排序](../design-patterns/render-layer-sort-permutation-indices.md)：显示坐标只负责找到路径，不能决定 safety 或替代授权身份。
- [差异化在执行，不在与竞品对立或省略功能](../product-decisions/differentiation-on-execution-not-opposition.md)：Risky 不预选、type-to-confirm 与默认 Trash 是不可退让的产品安全底线。
- [PR #45](https://github.com/snowzhaozhj/macCleaner/pull/45)：实现本次修复；文档撰写时仍为 OPEN。
