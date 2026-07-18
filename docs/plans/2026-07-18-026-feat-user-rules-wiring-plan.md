---
title: "feat: 用户叠加规则接线扫描 + 可见入口"
date: 2026-07-18
type: feat
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
product_contract_source: ce-plan-bootstrap
depth: standard
origin: docs/ideation/2026-07-05-beat-mole-product-directions.md（#2「规则外部化第一阶段」[打赢]，落地建议第 2 项）
---

# feat: 用户叠加规则接线扫描 + 可见入口

## Summary

把**已实现但未接线**的用户叠加规则（`~/.config/mc/rules.toml`）接进扫描主路径，并补上用户可见的入口/文档。

核心事实（已核实代码）：

1. **引擎已就绪**：`mc_core::rules` 里 `user_rules()`（读 `~/.config/mc/rules.toml`）、`user_rules_from_str()`（解析 + fail-closed 门禁 + 无条件 `preselect=false`）、`validate_user_rule()`（DirName 必配 root_markers）、`all_rules()`（内置 + 用户叠加）全部实现，且有测试（`rules.rs:891/936/953`）。
2. **接线缺口**：扫描两策略都只吃内置规则——`Scanner::scan_clean`（`scanner.rs:343`）用 `clean_rules()`，`Scanner::scan_purge`（`scanner.rs:352`）用 `purge_rules()`。用户即便写了 `rules.toml`，扫描也不采用；`all_rules()` 目前仅被只读诊断 `evidence_for_path`（`rules.rs:243`）使用。
3. **安全隔离已正确、不可动**：删除授权侧 `deletion_evidence_for_path`（`rules.rs:251`）刻意只信 `builtin_rules()`——「用户规则用于扩展扫描范围，不能作为任意路径降级为 Safe/Moderate 的依据」（`rules.rs:247-250`，已由 issue #23 fail-closed 学习锁定）。本计划**保持**这一隔离：用户规则只扩大**发现范围**，绝不参与删除授权降级。
4. **preselect 语义链完整**：`Meta::from_rule` 携带 `preselect`（`scanner.rs:195`），`to_scan_item` 用 `with_preselect` 传递（`scanner.rs:208`）；用户规则被 `user_rules_from_str` 强制 `preselect=false`，会正确流到 `ScanItem`——用户规则命中项**永不预选**，与 `--yes`/TUI 默认勾选解耦。

本计划：让 `scan_clean` 用「内置 clean 规则 + 用户规则里的 Exact 模式」扫描；`scan_purge` 用「内置 purge 规则 + 用户规则里的 DirName 模式」扫描；补 CLI 可见提示与 README/CONCEPTS 文档；沉淀一条 solution。无新 FFI、无 `unsafe`、不改删除授权的安全承诺。

**Product Contract preservation:** 无独立需求文档（solo 直接规划）；产品意图取自 beat-mole ideation #2「规则外部化第一阶段」，未改动任何既有产品范围。

---

## Problem Frame

- **痛点**：用户遇到一条内置规则没覆盖的缓存/开发产物（如冷门工具的缓存目录），当前只能等发版 + 等 Homebrew 更新。beat-mole #2 的价值主张是「热修一条误报/补一条规则从『发版』降到『改一行 TOML』」——但引擎写好了却没通电，用户写的 `rules.toml` 静默不生效。
- **为什么现在做**：这是 beat-mole 落地建议**第 2 项**（undo 是第 4 项、已出货）。引擎、门禁、测试都已就位，接线是**低边际成本兑现已投入的工程**——典型的「打赢」差异化（Mole 规则封闭在专有 App，我们可本地叠加 + 严格 lint）。
- **产品原则约束**（`STRATEGY.md` / `CONCEPTS.md` / `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`）：
  - 用户规则**永不预选**、永不被 `--yes`/默认勾选自动删除（`user_rules_from_str` 强制 `preselect=false`）。
  - 用户规则**不能**作为删除授权降级依据（删除侧继续只信 `builtin_rules()`）。
  - fail-closed：坏 TOML / 违反 DirName 守卫 → 整个用户规则文件跳过（返回空），扫描退化为纯内置，绝不崩溃、绝不半加载。

---

## Scope

### 本计划做

- `scan_clean` 接入用户规则的 **Exact** 模式（`clean` 策略是 Exact 路径合并遍历）。
- `scan_purge` 接入用户规则的 **DirName** 模式（`purge` 策略是 DirName 剪枝 + root_markers 守卫）。
- CLI `clean` / `purge` 在检测到并成功加载用户规则时，给一条克制的可见提示（加载了 N 条用户规则）。
- README + CONCEPTS.md 补「用户叠加规则」文档（文件位置、TOML 格式、安全约束、fail-closed 行为）。
- 沉淀一条 `docs/solutions/` 学习（用户规则接线的 pattern 分流与安全隔离）。

### Deferred to Follow-Up Work

- **签名可加载规则数据集**（beat-mole #2 第二阶段）——远程分发 + 签名校验，独立一轮。
- **社区 PR + CI 契约门禁**（#2 第三阶段）——`rules.rs` 契约测试机器把关外部贡献。
- **规则透明度页 / 公开安全边界文档**（信任资产轮）——已在方向问答里作为独立候选，不在本轮。
- **GUI 用户规则入口/编辑器**——本轮只接 core + CLI；GUI 可在 core 通电后另立项复用。
- **`能力分级 / disabled 默认`**（#2 描述里的「默认 disabled 能力分级」）——当前门禁已足够安全（DirName 守卫 + 强制 preselect=false + 删除侧隔离），能力分级是后续增强。

### Outside this product's identity

- 用户规则**不允许**声明任意路径为 Safe 并自动删除——这与「无静默删除」同一性冲突，由 `preselect=false` 强制 + 删除侧 `builtin_rules()` 隔离共同保证，本计划不打开任何绕过口。

---

## Requirements

- **R1** — `mc clean` 扫描结果包含用户规则里 Exact 模式命中的项（若 `~/.config/mc/rules.toml` 存在且通过门禁）。
- **R2** — `mc purge <dir>` 扫描结果包含用户规则里 DirName 模式命中的项（满足 root_markers 守卫）。
- **R3** — 用户规则命中项**永不预选**（`selected=false`），即便 TOML 里写了 `preselect=true`；`--yes` 不会自动删除它们。
- **R4** — 删除授权侧行为**不变**：`deletion_evidence_for_path` 继续只信 `builtin_rules()`；用户规则不能让任意路径的删除证据降级为 Safe/Moderate。
- **R5** — fail-closed：坏 TOML 或违反 DirName 守卫的规则文件被整体跳过，扫描退化为纯内置规则，进程不崩溃。
- **R6** — CLI 在成功加载用户规则时给一条可见提示；文件不存在时静默（零噪音）。
- **R7** — README + CONCEPTS.md 记录用户叠加规则的位置、格式、安全约束与 fail-closed 行为。

---

## Key Technical Decisions

### KTD1 — 按 pattern 类型分流用户规则到对应扫描策略，而非把 `all_rules()` 整块塞给某条扫描

`all_rules()` 是「内置 clean + 内置 purge + 用户规则」的混合集，但两条扫描策略的算法不同：`scan_clean`（`scan_with_rules`）只处理 `PathPattern::Exact`（`scanner.rs:367` 已 `if let PathPattern::Exact`），`scan_purge`（`scan_purge_dir`）只处理 `PathPattern::DirName`。用户规则可同时含两类 pattern。

**决策**：不改 `all_rules()`，而在扫描入口按策略取用户规则的相应子集：
- `scan_clean`：`clean_rules()` + `user_rules()`（`scan_with_rules` 本就只挑 Exact 模式，DirName 用户规则在此被自然忽略——安全且无副作用）。
- `scan_purge`：`purge_rules()` + `user_rules()`（`scan_purge_dir` 本就只按 DirName 剪枝，Exact 用户规则在此被自然忽略）。

**理由**：两条 `scan_*_dir` 内部已按 pattern 类型 filter，把完整 `user_rules()` 附加到任一策略都是安全的（不匹配的 pattern 被忽略），无需在接线层做 pattern 预分流——更简单、更少出错。`user_rules()` 每次扫描调一次（读一次文件），可接受（扫描本就是重 I/O 操作）。

**Alternative rejected**：让 `scan_clean` 调 `all_rules()`——会把内置 purge 规则（DirName）也带进 clean 扫描；虽然 `scan_with_rules` 只挑 Exact 会忽略它们，但语义误导（clean 不该"看见" purge 规则），且 `all_rules()` 的用途注释明确是"证据反查"。

### KTD2 — 删除授权侧一行不改，靠既有隔离保证安全

`deletion_evidence_for_path` / `deletion_evidence_for_paths` 继续调 `builtin_rules()`。用户规则扩大的是**扫描发现的项**（这些项本就带 `preselect=false`，需用户手动勾选），勾选后删除时若走 Analyze 式任意路径删除授权，仍按内置规则判等级——未匹配内置的按 Risky fail-closed。

**理由**：这正是 `analyze-unknown-path-deletion-fail-closed.md` 学习锁定的边界。扫描发现（可含用户规则）与删除授权（只信内置）解耦，是本产品安全模型的核心不变量。本计划**验证并保持**它，不新增授权口。

### KTD3 — CLI 提示克制、fail-closed 无噪音

加载用户规则的提示只在**成功加载 ≥1 条**时出现（如 `已加载 N 条用户叠加规则（~/.config/mc/rules.toml）`）。文件不存在 → 静默（绝大多数用户无此文件，零噪音）。解析/门禁失败 → `user_rules_from_str` 已 `log::error!`，CLI 层不重复播报（避免双重错误信息）。

---

## Implementation Units

### U1. `scan_clean` / `scan_purge` 接入用户规则

**Goal:** 让两条扫描策略在内置规则基础上附加用户规则，使用户 `rules.toml` 命中项进入扫描结果。

**Requirements:** R1, R2, R3, R5

**Dependencies:** 无（引擎已就绪）

**Files:**
- `crates/core/src/scanner.rs`（修改 `scan_clean` `:342`、`scan_purge` `:348`）
- `crates/core/src/scanner.rs`（新增/扩展测试）

**Approach:**
- `scan_clean`：`let mut rules = clean_rules(); rules.extend(user_rules());` 后传给 `scan_with_rules`。`scan_with_rules` 内部 `if let PathPattern::Exact` 会自然只取 Exact 用户规则，DirName 用户规则在 clean 扫描被忽略（预期行为）。
- `scan_purge`：`let mut rules = purge_rules(); rules.extend(user_rules());` 后传给 `scan_purge_dir`。`scan_purge_dir` 按 DirName 剪枝，Exact 用户规则在 purge 扫描被忽略（预期行为）。
- `user_rules()` 已优雅降级（文件不存在/读失败/门禁不过 → 空 Vec），无需在此加防御。
- 依赖 `use` 已含 `clean_rules`/`purge_rules`，需补 `user_rules` 导入。

**Patterns to follow:** `all_rules()`（`rules.rs:223`）的 `rules.extend(user_rules())` 追加写法；`Meta::from_rule` 的 `preselect` 传递链（`scanner.rs:195/208`）无需改动，用户规则的 `preselect=false` 自动生效。

**Test scenarios:**
- **Happy path (clean)**：注入一条含 Exact 模式、指向临时目录里真实文件的用户规则，`scan_with_rules` 结果包含该项。（用 `user_rules_from_str` 造规则 + 手动 extend 到 `clean_rules()`，或对 `scan_with_rules` 直接传混合规则集——后者更贴近接线后行为。）
- **Happy path (purge)**：注入一条含 DirName + root_markers 的用户规则，在临时目录构造满足守卫的目录树，`scan_purge_dir` 剪枝命中该目录。
- **R3 preselect**：用户规则 TOML 写 `preselect=true`，经 `user_rules_from_str` 后命中项 `ScanItem.selected == false`（`user_rules_from_str_forces_preselect_false` 已覆盖加载层；此处验证接线后 `ScanItem` 层）。
- **R5 fail-closed**：用户规则文件含违反 DirName 守卫的规则 → 扫描退化为纯内置结果（等于不接用户规则时的结果），不 panic。
- **Cross-strategy isolation**：只含 DirName 模式的用户规则在 `scan_clean` 里不产生任何额外项（被 Exact filter 忽略）；只含 Exact 的用户规则在 `scan_purge` 里不产生额外项。
- **Test expectation 说明**：`user_rules()` 读真实 `~/.config`，单测不便触真实 home——测试应针对 `scan_with_rules`/`scan_purge_dir`（接受显式 rules 切片）注入混合规则集，验证接线**行为**；`scan_clean`/`scan_purge` 薄封装本身（一行 extend）由集成层的现有 smoke 覆盖即可。

**Verification:** `cargo test -p mc-core scanner::` 全绿；注入用户规则的新测试证明命中项出现且 `selected=false`。

### U2. CLI 加载提示（clean / purge）

**Goal:** 成功加载用户规则时给一条可见、克制的提示。

**Requirements:** R6

**Dependencies:** U1

**Files:**
- `crates/cli/src/commands/clean.rs`（`Engine::scan_clean` 调用点 `:74` 附近）
- `crates/cli/src/commands/purge.rs`（对应 scan 调用点）

**Approach:**
- 扫描前调 `mc_core::rules::user_rules()` 拿数量（或暴露一个轻量 `user_rules_count()` / 复用 `user_rules().len()`）；`len() > 0` 时打印提示。
- 提示走 CLI 既有的非进度输出通道（普通 stdout/stderr，`--json` 模式下应并入 JSON meta 或抑制——与既有 CLI 输出约定一致，实现时查 `clean.rs` 现有输出如何处理 `--json`）。
- 文件不存在 → `user_rules()` 返回空 → 不打印。

**Patterns to follow:** `clean.rs` 现有的扫描前/后信息输出（进度条 reporter 之外的普通提示）。

**Test scenarios:**
- **Test expectation: 手动/集成为主** — CLI 输出提示的单测价值低（需捕获 stdout + 造真实 `~/.config` 文件）。以 `user_rules().len()>0` 分支的存在性 + 一次手动验证（真机放一个 `rules.toml` 跑 `mc clean --dry-run` 看提示）为准。若 `clean.rs` 已有可注入的输出抽象，则加一条「加载 N 条 → 提示含 N」的单测。
- **零噪音**：无 `rules.toml` 时 `mc clean` 输出不含用户规则提示（手动验证）。
- **--json 不破坏**：`mc clean --json` 输出仍是合法 JSON（提示不污染 JSON 流——实现时确认）。

**Verification:** 真机 `mc clean --dry-run`（有/无 `rules.toml` 两态）观察提示；`mc clean --json` 输出经 `jq` 校验合法。

### U3. 文档 + solution 沉淀

**Goal:** 用户能查到怎么写 `rules.toml`；团队沉淀接线的安全推理。

**Requirements:** R7

**Dependencies:** U1（行为定稿后再写文档）

**Files:**
- `README.md`（新增「用户叠加规则」小节）
- `CONCEPTS.md`（补「用户叠加规则」条目——若该术语在 plan 落地后成为项目词汇）
- `docs/solutions/`（新增一条学习，类别 `security-issues` 或新建 `rules-extensibility`）

**Approach:**
- README：文件位置 `~/.config/mc/rules.toml`、最小 TOML 示例（一条 Exact + 一条 DirName+root_markers）、安全约束（永不预选、fail-closed、不能降级删除授权）、DirName 必配 root_markers 的原因。
- CONCEPTS.md：按现有条目格式补「用户叠加规则」定义（区别于内置规则：扫描扩展 vs 删除授权隔离）。
- solution：记录 KTD1（pattern 分流靠策略内既有 filter）+ KTD2（删除授权隔离不变量），带 `module`/`tags`/`problem_type` frontmatter。

**Patterns to follow:** 现有 `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md` 的结构与 frontmatter；CONCEPTS.md 现有条目格式。

**Test scenarios:** Test expectation: none — 纯文档，无行为变更。人工核对示例 TOML 能被 `user_rules_from_str` 成功解析（可加一条「README 示例 TOML 通过门禁」的单测把文档示例钉死为契约，防文档漂移）。

**Verification:** README 示例 TOML 复制到临时文件，`user_rules()` 能加载且 `validate_user_rule` 通过。

---

## System-Wide Impact

- **扫描性能**：`user_rules()` 每次 `scan_clean`/`scan_purge` 多一次文件 stat + （存在时）一次读 + 解析。绝大多数用户无此文件（`path.exists()` 短路），成本可忽略；有文件时规则条数量级极小（个位数），解析成本相对全盘扫描可忽略。
- **删除安全**：无变化（KTD2）——本计划的安全论证核心是「验证隔离仍成立」，不是「新增保护」。
- **TUI/GUI**：本轮不接 TUI/GUI 入口，但 core 通电后 TUI 走 `Engine::scan_clean` 会自动看到用户规则命中项（带 `preselect=false`，符合 TUI「Risky/非预选需手动勾」语义）。这是免费的正向溢出，需在文档里说明 TUI 也会看到用户规则项。

---

## Risks & Mitigations

- **风险：用户规则被误当删除授权依据**。缓解：KTD2 删除侧零改动 + U1 测试显式验证 `selected=false` + 现有 `deletion_evidence_for_path` 用 `builtin_rules()` 的测试继续绿。
- **风险：DirName 用户规则无 root_markers 导致整树误报**。缓解：`validate_user_rule` 已 fail-closed 拒绝（`rules.rs:169`），整个文件跳过；U1 测试覆盖。
- **风险：`--json` 输出被提示污染**。缓解：U2 明确要求 `--json` 下提示并入 meta 或抑制，实现时查既有约定 + 测试校验 JSON 合法。

---

## Verification Contract

- `cargo test -p mc-core` 全绿（含 U1 新增扫描接线测试、既有 `deletion_evidence` 隔离测试）。
- `cargo clippy --all-targets` 无新告警（pedantic 全开）。
- 真机 `mc clean --dry-run` 有/无 `~/.config/mc/rules.toml` 两态：有则出现加载提示且结果含用户规则项（`selected=false`，`--yes` 不删）；无则零噪音。
- `mc clean --json` 输出经 `jq` 校验为合法 JSON。
- README 示例 TOML 能被 `user_rules()` 成功加载（U3 契约测试或手动验证）。

## Definition of Done

- R1–R7 全部满足并有测试或明确的手动验证记录。
- 用户规则进入 `scan_clean`/`scan_purge` 结果，命中项 `preselect=false`。
- 删除授权侧行为零变化，隔离测试继续绿。
- fail-closed 行为经测试验证（坏规则 → 退化纯内置，不崩溃）。
- README + CONCEPTS.md + 一条 solution 落地。
- clippy 干净、全测试绿。

---

## Sources & Research

- `docs/ideation/2026-07-05-beat-mole-product-directions.md`（#2 规则外部化，落地建议第 2 项）
- `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`（删除授权 fail-closed 隔离，KTD2 的直接依据）
- 代码核实：`crates/core/src/rules.rs`（`user_rules`/`all_rules`/`validate_user_rule`/`deletion_evidence_for_path`）、`crates/core/src/scanner.rs`（`scan_clean:342`/`scan_purge:348`/`scan_with_rules:357`/`Meta:186`）、`crates/cli/src/commands/clean.rs`（`scan_clean` 调用 `:74`）
