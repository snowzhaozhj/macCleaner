---
title: "feat: 孤儿残留扫描（反向卸载）+ mc orphans"
date: 2026-07-19
type: feat
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
product_contract_source: ce-plan-bootstrap
depth: standard
origin: docs/ideation/2026-07-05-beat-mole-product-directions.md（方向 #5「孤儿残留扫描（反向卸载）」；落地建议第 5 项；roadmap issue #27）
---

# feat: 孤儿残留扫描（反向卸载）+ mc orphans

## Summary

新增**反向卸载**能力：遍历 `~/Library` 标准子目录，找出**父 App 已不存在**的 bundle-id 形态残留（用户装了又删的应用留下的 orphan），供用户回收。与现有 `mc uninstall`（**正向**：给定已装 App → 找它的残留）互补。

核心事实（已核实代码）：

1. **现只有正向路径**：`AppResolver::find_leftovers(bundle_id)`（`crates/core/src/app_resolver.rs:244`）给定一个 bundle_id、在 8 个 `~/Library` 子目录里前缀匹配残留。反向扫描（枚举残留 → 反查父 App 是否还在）**不存在**。
2. **前置安全债已还清**：issue #25 方案 B 已合并——`USER_DATA_SUBDIRS`（`Application Support`/`WebKit`/`HTTPStorages`/`Saved Application State`，`app_resolver.rs:29`）派生的残留标 `Moderate` + `preselect=false` + 非空证据文案；其余（Caches/Preferences/Logs 等）标 `Safe`。本计划**复用**这套分级，且对孤儿场景**收紧**（见 KTD2）。
3. **已装 App 集合可得**：`AppResolver::list_apps()`（`app_resolver.rs:39`）返回 `Vec<AppInfo>`，每项含 `bundle_id: Option<String>`——构建"当前已安装 bundle-id 集合"的现成数据源。
4. **Engine facade 平价委托**：`Engine::list_apps` / `Engine::find_leftovers`（`engine.rs:32/40`）是无逻辑委托。新引擎方法沿用同样的 facade 平价约定。
5. **CLI 子命令扩展点清晰**：`Commands` enum（`crates/cli/src/main.rs:27`）+ `commands/` 目录一命令一文件；`uninstall.rs` 是残留清理交互 + `Engine::clean` 的完整模板。

本计划：core 加 `scan_orphans()` 反向扫描引擎（bundle-id 集合 + 系统预留黑名单 + 龄阈值 + 分级收紧）；CLI 加 `mc orphans` 子命令（列出候选 + 证据 + 确认 + 移废纸篓）；补文档 + 沉淀一条 solution。无新 FFI、无 `unsafe`、不改删除授权安全承诺。

**Product Contract preservation:** 无独立需求文档（solo 直接规划）；产品意图取自 beat-mole ideation 方向 #5 与 roadmap issue #27，未改动任何既有产品范围。

---

## Problem Frame

- **痛点**：用户装过一个 App、后来把 `.app` 拖进废纸篓删了，但 `~/Library` 里的 Caches/Preferences/Application Support 残留**留了下来**——正向 `mc uninstall` 找不到它们（App 已不在列表里，无从选起）。这些"没有主人的残留"正是普通用户最摸不着、CleanMyMac/AppCleaner 招牌覆盖的场景。Mole `mo clean` 也含已卸载残留，故此项是**追平**（roadmap 标注 [追平]）——填能力缺口，不是差异化赌注。
- **为什么现在做**：beat-mole 落地建议**第 5 项**，唯一前置（#25 app_resolver 残留分级复议，方案 B）已随 PR #29 合并。引擎侧正向 `find_leftovers` 是强参照，安全分级 rubric 已就位——反向扫描是低边际成本兑现。
- **产品原则约束**（`STRATEGY.md` / `CONCEPTS.md` / `crates/core/src/models.rs` SafetyLevel 文档 / `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`）：
  - **误杀真实用户数据是本功能唯一的雷区**：共享 bundle-id 前缀（`com.apple.*` 系统预留、`com.google.*` 下多产品共用目录）、系统自带的常驻残留目录，绝不能当孤儿删。
  - 孤儿残留**永不默认预选**——App 已卸载但用户可能**故意保留数据**（如想以后重装恢复配置）。默认删会毁信任。
  - 删除默认**移废纸篓可恢复**；沿用现有 `DeleteMode`。
  - fail-closed：无法确定父 App 是否存在（bundle-id 解析不出、路径读不了）→ **不列为孤儿**（宁可漏报，不可误杀）。

---

## Scope

### 本计划做

- core `AppResolver::scan_orphans()`：反向扫描 `~/Library` 标准子目录，产出父 App 已不存在的 bundle-id 残留 `Vec<ScanItem>`。
- 系统预留 / 共享前缀黑名单（`com.apple.*` 等），防误杀。
- 龄阈值（默认 30 天，按 mtime）——刚删的残留可能是用户临时清理，给缓冲期，减少误报。
- 分级收紧：孤儿残留一律 `preselect=false`；`USER_DATA_SUBDIRS` 派生项 `Moderate` + 证据文案，其余 `Safe`（复用 #25 rubric）。
- Engine facade 平价方法 `Engine::scan_orphans()`。
- CLI `mc orphans` 子命令：列出候选（含证据文案）+ `--yes`/交互确认 + `--dry-run` + `--json` + 移废纸篓。
- README + CONCEPTS.md 补「孤儿残留 / 反向卸载」文档（定义、黑名单、龄阈值、安全约束、fail-closed）。
- 沉淀一条 `docs/solutions/` 学习（反向卸载的父 App 存在性判定 + 误杀防线）。

### Deferred to Follow-Up Work

- **GUI / TUI 孤儿扫描入口**——本轮只接 core + CLI；core 通电后另立项复用（GUI 复用 move 7 的 uninstall 入口模式）。
- **孤儿残留的规则草稿喂养**（把未识别的大残留一键起草成 `~/.config/mc/rules.toml` 条目）——与 beat-mole #6 Analyze 归因合并到那一轮。
- **LaunchAgents/LaunchDaemons 活跃 plist 的反向卸载**（残留的登录项/守护进程 plist 仍被 launchd 引用）——涉及 app 生命周期，超出纯文件残留范围。
- **跨用户 / 系统级 `/Library`（非 `~/Library`）孤儿**——需提权，风险面大，暂缓。

---

## Requirements

- **R1**：给定当前 `~/Library` 状态，`scan_orphans()` 返回**仅**父 App 不在已安装集合、且不在系统预留黑名单、且龄 ≥ 阈值的 bundle-id 残留。
- **R2**：任何无法确定父 App 存在性的候选（bundle-id 无法从条目名析出、目录读取失败）**不得**被列为孤儿（fail-closed，宁漏报不误杀）。
- **R3**：所有孤儿残留 `preselect=false`；`USER_DATA_SUBDIRS` 派生项为 `Moderate` + 非空证据文案，其余为 `Safe`（分级与 #25 一致，预选一律关闭）。
- **R4**：`mc orphans` 列出候选、展示证据、`--dry-run` 只预览、交互或 `--yes` 后移废纸篓删除、`--json` 输出结构化列表、`--permanent` 走永久删除。
- **R5**：系统预留 / 共享 bundle-id 前缀（至少 `com.apple.`）绝不出现在孤儿候选中，有测试锁定。

---

## Key Technical Decisions

### KTD1：反向匹配的身份键 = 条目名析出的 bundle-id，比对 `list_apps()` 集合

`~/Library/<subdir>/` 下的残留条目名通常是 `com.vendor.App`、`com.vendor.App.plist`、`com.vendor.App-hash` 形态。反向判定：从条目名**析出候选 bundle-id 前缀**（剥掉 `.plist` 扩展、`.`/`-` 后缀 hash），若该前缀**不匹配** `list_apps()` 里任何已安装 App 的 bundle-id（大小写不敏感、前缀关系双向判断，与正向 `find_leftovers` 的匹配规则对称）→ 候选孤儿。

- **Rationale**：正向 `find_leftovers`（`app_resolver.rs:269-272`）已定义了"bundle_id ↔ 条目名"的匹配规则（相等 / `bid.` 前缀 / `bid-` 前缀）；反向就是它的补集。复用同一规则保证正反一致，不引入第二套匹配语义。
- **Alternatives**：解析每个残留内部的元数据反查 bundle-id（过重、多数残留无元数据）；用 `mdls`/Spotlight（依赖索引、慢、可能关索引）。均否。

### KTD2：孤儿场景比正向卸载**更保守**——一律不预选

正向 `mc uninstall` 里非用户数据残留（Caches 等）是 `Safe`+预选（用户已明确选中要卸的 App，残留跟着删是预期）。孤儿场景**没有这个明确意图**——用户没说要删任何东西，是工具主动发现的。故：

- 孤儿残留**全部 `preselect=false`**（含 Safe 项），无论子目录类型。
- 分级仍按 #25 rubric（USER_DATA → Moderate + 证据，其余 Safe），但预选独立关闭。

- **Rationale**：`models.rs` 的 `selected = safety != Risky && preselect`；孤儿场景把 preselect 统一关掉，等于"永不默认删、永不 `--yes` 自动删，用户须逐项手动勾"。呼应约束"App 已卸载但用户可能故意保留数据"。这与 beat-mole #6 的"未知用户文件不能默认 Safe/选中"是同一信任原则。
- **Trade-off**：`mc orphans --yes` 不会删任何东西（无预选项）——这是**刻意**的，孤儿删除必须是显式选择。CLI 需明确提示"孤儿默认不勾选，请指定要删的项"。

### KTD3：系统预留黑名单 + 龄阈值双闸门防误杀

- **黑名单**：至少屏蔽 `com.apple.` 前缀（系统组件残留常年存在于 `~/Library`，父"App"是系统而非可卸载 App）。黑名单是常量数组，可扩展（如 `com.google.` 下的共享目录按需评估）。
- **龄阈值**：默认 30 天（按残留目录 mtime）。刚删 App 的残留可能是用户临时操作、或马上要重装 → 给缓冲期。阈值是常量，测试可注入。

- **Rationale**：roadmap 明确"30 天以上孤儿候选"+"共享 bundle-id、系统预留目录"是误杀源。两道闸门把误报压到最低，代价是可能漏掉刚删/系统边缘的残留——符合 fail-closed 取向。
- **Alternatives**：只靠"父 App 不存在"单条件 → 会把系统常驻残留、刚删残留全列出，误杀风险高。否。

### KTD4：新独立子命令 `mc orphans`，不塞进 `mc uninstall`

`mc uninstall` 的心智是"选一个已装 App 卸载"，孤儿是"扫全局找无主残留"——两种不同的用户意图和交互流。合并会让 uninstall 的编号选择流与全局扫描流打架。

- **Rationale**：与 CLAUDE.md「统一的是入口菜单不是结果集，勿字面合并不同心智模型的动作」一致（该原则原用于 GUI move 7，同样适用 CLI）。独立命令交互更清晰。
- **Alternatives**：`mc uninstall --orphans` 标志复用同文件 → 交互流分叉、`run()` 里两套逻辑，可读性差。否。

---

## Implementation Units

### U1. core `scan_orphans()` 反向扫描引擎

**Goal**：在 `AppResolver` 加反向扫描，产出父 App 已不存在的孤儿残留列表。

**Requirements**：R1, R2, R3, R5

**Dependencies**：无（复用现有 `list_apps` / `LEFTOVER_SUBDIRS` / `USER_DATA_SUBDIRS` / `calc_app_size`）。

**Files**：
- `crates/core/src/app_resolver.rs`（新增 `scan_orphans()` + 私有辅助 `extract_bundle_id_prefix()` / `is_reserved_prefix()` / 龄判定；新增系统预留黑名单常量 + 龄阈值常量；测试）

**Approach**：
- 构建已安装 bundle-id 集合：`list_apps()` → 收集 `Some(bundle_id).to_lowercase()` 到 `HashSet<String>`。
- 遍历 `LEFTOVER_SUBDIRS` 每个 `~/Library/<subdir>/` 条目：
  1. 从条目名析出候选 bundle-id 前缀（剥 `.plist` 等扩展、`.`/`-` 后 hash 段）。析不出（不含 `.` 的普通名，如 `Caches/Google/`）→ **跳过**（R2 fail-closed，不当孤儿）。
  2. 前缀命中系统预留黑名单（`com.apple.` 等）→ 跳过（R5）。
  3. 候选前缀与已装集合任一 bundle-id 满足正向匹配规则（相等 / 互为 `.`/`-` 前缀）→ **父 App 仍在**，跳过。
  4. 残留目录 mtime 龄 < 阈值 → 跳过（缓冲期）。
  5. 全部通过 → 是孤儿。按子目录归类：`USER_DATA_SUBDIRS` → `Moderate` + 证据文案 + `with_preselect(false)`；其余 → `Safe` + `with_preselect(false)`（KTD2，Safe 也关预选）。
- 复用 `calc_app_size` 计体积；证据文案沿用 `find_leftovers` 措辞，category 用「孤儿残留 (<subdir>)」以便与正向残留区分。

**Patterns to follow**：`find_leftovers`（`app_resolver.rs:244-317`）的目录遍历 + 匹配 + 分级结构；`USER_DATA_SUBDIRS` 分级分支（`:296-305`）。

**Test scenarios**：
- 已装 App 的残留**不**被列为孤儿：构造临时 `~/Library` 替身（可注入 library 根 + 已装集合的测试内核），放 `com.installed.App` 残留 + 已装集合含 `com.installed.App` → 结果不含它。
- 父 App 不存在的残留**被**列为孤儿：放 `com.gone.App`，已装集合不含 → 结果含它，`preselect=false`。
- **R5**：放 `com.apple.Safari` 残留、已装集合不含 Safari → 因黑名单被排除，**不**出现在孤儿列表。
- **R2 fail-closed**：条目名不含 `.`（如 `Google` 目录）→ 析不出 bundle-id，跳过、不列为孤儿。
- **R3 分级**：`Application Support/com.gone.App`（USER_DATA）→ `Moderate` + 非空 impact/recovery + `!selected`；`Caches/com.gone.App`（非 USER_DATA）→ `Safe` + `!selected`（Safe 也不预选，KTD2）。
- **龄阈值**：mtime 在阈值内的孤儿残留被跳过；超阈值的被列出（用可注入阈值或临时改 mtime）。
- 无 panic / 空环境稳健：`~/Library` 子目录不存在时跳过、返回空不崩。

**Verification**：`cargo test -p mc-core app_resolver::` 全绿；新用例覆盖上述每条；`cargo clippy -p mc-core --all-targets` 无警告。

**Execution note**：孤儿判定是纯逻辑 + 文件系统读，先写可注入 library 根与已装集合的测试内核（对称于现有 `scan_apps_in_dirs` 的可注入目录模式），再让 `scan_orphans()` 薄封装真实 `~/Library` + `list_apps()`。测试不依赖真机 `~/Library` 内容。

### U2. Engine facade 平价方法

**Goal**：`Engine::scan_orphans()` 无逻辑委托 `AppResolver::scan_orphans()`，供 CLI（及后续 GUI/TUI）单一入口调用。

**Requirements**：R1（对外暴露）

**Dependencies**：U1

**Files**：
- `crates/core/src/engine.rs`（新增 `pub fn scan_orphans() -> Vec<ScanItem>` + facade 平价测试）

**Approach**：照 `Engine::find_leftovers`（`engine.rs:40`）的委托 + 文档注释风格；一行委托，无逻辑。

**Patterns to follow**：`engine.rs:36-42` 的 facade 平价方法 + 其下的平价委托测试（`:83-90`）。

**Test scenarios**：
- facade 平价：`Engine::scan_orphans()` 在同环境下与 `AppResolver::scan_orphans()` 等价（委托生效、不 panic）。空环境返回不崩。

**Verification**：`cargo test -p mc-core engine::` 全绿。

### U3. CLI `mc orphans` 子命令

**Goal**：新增 `mc orphans` 命令：扫描孤儿残留、展示证据、确认后移废纸篓。

**Requirements**：R4, R3（预选行为在 CLI 可见）

**Dependencies**：U2

**Files**：
- `crates/cli/src/main.rs`（`Commands` enum 加 `Orphans` 变体 + `match` 分派）
- `crates/cli/src/commands/orphans.rs`（新建）
- `crates/cli/src/commands/mod.rs`（挂载模块，若存在集中挂载）

**Approach**：
- 以 `uninstall.rs` 为模板，但**去掉编号选择流**（孤儿是全局扫描非选一个 App）。
- 流程：`Engine::scan_orphans()` → 空则提示"未发现孤儿残留"→ `--json` 直接输出 → 否则列表打印（路径 + 体积 + USER_DATA 项的 ⚠ impact / ↩ recovery 证据，照 `uninstall.rs:105-115`）→ `--dry-run` 到此为止 → 否则**因孤儿全不预选，须交互勾选或提示**：默认全不选，让用户输入要删的编号（逗号/范围）或 `a` 全选或 `q` 取消；`--yes` 时**不自动删**（无预选项），打印提示"孤儿默认不勾选，请交互指定或省略 --yes"（KTD2 的刻意约束）→ 选中项 `Engine::clean` 移废纸篓（`--permanent` 例外）。
- 复用 `uninstall.rs` 的 `CliReporter`（可上提到共享处或各自实现，保持简单则各自留一份）。

**Patterns to follow**：`uninstall.rs`（列表 + 证据展示 + 确认 + `Engine::clean` + `DeleteMode` 分支）；`main.rs:27-73` 的 enum 变体 + 分派。

**Test scenarios**：
- `Test expectation: none -- CLI 交互命令，逻辑主要是 I/O 与用户输入解析；核心判定已由 U1 单测覆盖。` 若编号解析（逗号/范围/`a`/`q`）抽成纯函数，则对该解析函数补：空输入→空选择、`a`→全选、`1,3`→选中第 1/3 项、越界编号→报错、`q`→取消。
- `mc orphans --json` 在无孤儿时输出空数组 `[]` 而非报错（若抽出可测则覆盖）。

**Verification**：`cargo build -p mc`；手动 `cargo run -p mc -- orphans --dry-run` 与 `--json` 在开发机跑通、输出合理（真机 `~/Library` 存在孤儿时列出、无则提示未发现）；`cargo clippy -p mc --all-targets` 无警告。

**Execution note**：优先把"编号选择解析"抽成纯函数以获得单测覆盖；交互 I/O 主体走 `--dry-run`/`--json` 的运行时冒烟验证。

### U4. 文档 + solution 沉淀

**Goal**：补用户文档与可复用学习。

**Requirements**：R1-R5（对外说明契约）

**Dependencies**：U1, U3

**Files**：
- `README.md`（`mc orphans` 用法段 + 与 `mc uninstall` 区别 + 安全约束）
- `CONCEPTS.md`（新增「孤儿残留 / 反向卸载」词条：定义、黑名单、龄阈值、永不预选、fail-closed）
- `docs/solutions/security-issues/orphan-leftover-scan-false-positive-defenses.md`（新建：反向卸载的父 App 存在性判定 + 三道误杀防线（fail-closed 析取、系统预留黑名单、龄阈值）+ 与正向 `find_leftovers` 的匹配对称性）

**Approach**：README/CONCEPTS 沿用既有词条格式与中文风格；solution 带 `module` / `tags` / `problem_type` frontmatter（参照 `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md` 与 `user-overlay-rules-wiring.md` 的结构）。

**Patterns to follow**：`docs/solutions/security-issues/*.md` 现有两篇的 frontmatter + 叙事结构；CONCEPTS.md 现有词条格式。

**Test scenarios**：`Test expectation: none -- 纯文档。` README 若含命令示例，确保与实际 `mc orphans --help` 一致（手动核对）。

**Verification**：文档与实现一致（命令名、标志、行为措辞）；`cargo run -p mc -- orphans --help` 输出与 README 描述吻合。

---

## System-Wide Impact

- **安全模型**：不改删除授权侧（仍只信 `builtin_rules()`）；孤儿分级完全落在 `SafetyLevel` 现有决策树内，`preselect=false` 是既有语义的更保守应用。无新信任轴。
- **无新依赖 / 无 FFI / 无 unsafe**：纯文件系统读 + 现有 `std::fs` / `humansize` / `serde_json`。
- **CLI 表面扩展**：新增一个子命令，`--help` 输出变化——属外部契约面（issue #27 追平项，预期内）。
- **性能**：`scan_orphans` 遍历 8 个 `~/Library` 子目录 + 对每个孤儿 `calc_app_size`；量级与正向 `find_leftovers` 同级，无并发需求（残留数量有限）。

---

## Risks & Mitigations

| 风险 | 缓解 |
| --- | --- |
| **误杀真实用户数据**（共享 bundle-id、系统预留目录） | 三道闸门：fail-closed 析取（析不出 bundle-id 不列）、系统预留黑名单（`com.apple.` 等）、龄阈值（默认 30 天）；`preselect=false` 保证永不自动删；R5 测试锁定黑名单。 |
| **漏报刚删残留** | 刻意取舍（龄阈值缓冲期）——孤儿是回收非必删，漏报可再扫；误杀不可逆代价更高。文档说明阈值语义。 |
| **黑名单不全**（`com.google.` 等共享前缀） | 首版只硬保 `com.apple.`；黑名单是可扩展常量数组，solution 记录扩展判据，后续按真机误报反馈追加。 |
| **`mc orphans --yes` 用户预期"全删"落空** | KTD2 刻意约束（孤儿无预选）；CLI 明确提示"孤儿默认不勾选"，避免静默无操作的困惑。 |

---

## Assumptions（headless 规划推断，未经用户确认）

- **A1**：命令名用 `mc orphans`（独立子命令），而非 `mc uninstall --orphans`（KTD4）。若用户偏好后者，U3 改为标志复用同文件。
- **A2**：龄阈值默认 **30 天**（取自 roadmap "30 天以上孤儿候选"表述）。可后续加 `--min-age-days` 标志调整；首版硬编码常量。
- **A3**：系统预留黑名单首版只硬保 `com.apple.` 前缀；其余共享前缀（`com.google.` 等）按真机误报反馈迭代，不在首版穷举。
- **A4**：孤儿残留 category 文案用「孤儿残留 (<subdir>)」以区别正向「应用残留 (<subdir>)」；若用户希望统一措辞，调 U1 的 category 字符串。

---

## Definition of Done

- `cargo test`（含 `-p mc-core`）全绿，U1/U2 新用例覆盖 R1/R2/R3/R5 每条判定。
- `cargo clippy --all-targets` 无警告（pedantic）。
- `cargo run -p mc -- orphans --dry-run` / `--json` / `--help` 在开发机运行合理。
- README + CONCEPTS.md + solution 三处文档与实现一致。
- 不触碰删除授权安全承诺（删除侧仍只信内置规则）；无新 unsafe/FFI。

---

## Sources & Research

- `docs/ideation/2026-07-05-beat-mole-product-directions.md` 方向 #5（反向卸载，[追平]）+ 落地建议第 5 项。
- roadmap issue #27（「留在文档、暂不建 issue」列孤儿残留，标注"需先决 #25"，#25 已合并 PR #29）。
- `crates/core/src/app_resolver.rs`（正向 `find_leftovers` + #25 分级 rubric，本计划的强参照）。
- `docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`（fail-closed / 未知路径不默认 Safe 的信任原则）。
- 外部研究：**未运行**（solo pipeline 模式；本地正向 `find_leftovers` 是充分参照，安全 rubric 已由 #25 锁定，无未决外部选型）。
