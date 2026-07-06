# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

macCleaner（二进制名 `mc`）是一个快速、安全、零遥测的 Mac 清理工具：CLI + TUI 共享同一核心引擎。产品定位与目标用户见 `STRATEGY.md`，领域词汇见 `CONCEPTS.md`（改动安全模型、命令语义、标记/删除相关代码前务必先读）。

## 常用命令

```bash
cargo build                          # 构建整个 workspace
cargo build --release                # 发布构建（LTO + strip，见根 Cargo.toml profile）
cargo run -p mc                      # 运行 TUI（无子命令即进入交互界面）
cargo run -p mc -- clean             # 运行某个 CLI 子命令
cargo run -p mc -- purge ~/code
cargo run -p mc -- analyze ~ --threshold 100
cargo run -p mc -- clean --dry-run   # 预览不删除（别名 --preview）

cargo test                           # 全部单测
cargo test -p mc-core                # 单个 crate
cargo test -p mc-core rules::        # 单个模块
cargo test scan_clean_streamed       # 单个测试（按名字子串）

cargo clippy --all-targets           # lint（pedantic 全开，见下）
cargo clippy --fix --allow-dirty --allow-staged
cargo bench -p mc-core               # criterion 基准（scan_purge_bench）
```

CLI 全局参数（`crates/cli/src/main.rs`）：`--dry-run`、`-y/--yes`（跳过确认，删除**已预选项**即非 Risky；help/注释里的「仅 safe」措辞不准）、`--permanent`（永久删除而非移废纸篓）、`--json`。

## 架构

三个 crate 的 workspace，UI 层（CLI/TUI）共享 `mc-core` 引擎：

- **`crates/core`（`mc-core`）** — 无 UI 依赖的扫描/清理引擎。
- **`crates/cli`（`mc`）** — clap 子命令 + 二进制入口；无子命令时委托给 `mc_tui::run()`。
- **`crates/tui`（`mc-tui`，lib 名 `mc_tui`）** — ratatui 交互界面。

### 核心数据流：Engine + ProgressReporter

所有 UI 都经 `engine::Engine`（`scan_clean` / `scan_purge` / `scan_uninstall` / `clean` / `dry_run`）这一薄 facade 调用核心。引擎与 UI 通过 **`progress::ProgressReporter` trait 解耦**：核心只 `on_event(ProgressEvent)` 并查询 `is_cancelled()`，不知道对端是 CLI 进度条还是 TUI。

- CLI：命令内实现 reporter（indicatif 进度条）。
- TUI：`reporter::TuiReporter` 把事件通过 `crossbeam-channel` 送回主线程。扫描/清理跑在 `thread::spawn` 后台线程（用 `catch_unwind` 包裹，保证 panic 也回发 `Error` 事件解卡），主事件循环 `select` 合并键盘/进度/analyze/sort 四路 channel。
- **取消是协作式**：`TuiReporter` 在 `cancelled` 置位后直接丢弃事件，避免残留事件在返回菜单/下次扫描时污染 `scan_result`（`reporter.rs` 有专门测试）。

磁盘分析（Analyze）**不走** `ProgressReporter`，而是独立的 `AnalyzeEvent` channel + `IncrementalTreeBuilder` 增量建树，因为它是流式导航视图而非规则匹配清理。

### 规则驱动的扫描（`core/src/rules.rs` + `*_rules.toml`）

清理项由两张 TOML 规则表定义，`include_str!` 编译进二进制：`clean_rules.toml`（系统缓存/日志）、`purge_rules.toml`（开发产物）。规则含 `patterns`（`Exact` 绝对/home 相对路径，或 `DirName` 目录名）、`safety`、`category`、`impact`/`recovery` 证据文案、`root_markers`（项目根守卫）、`preselect`。

两种扫描策略（`core/src/scanner.rs`）：

- **Clean（`scan_with_rules`）**：`Exact` 路径合并重叠后遍历，文件按**最长前缀**归入最具体规则；边扫边流式上报各分类 **size 增量（delta）**，TUI 按 `(category, path)` 合并累加，让列表边扫边填充。
- **Purge（`scan_purge_dir`）**：`DirName` 剪枝遍历——命中目录名 + 满足 `root_markers`（如 `node_modules` 旁有 `package.json`、`target` 旁有 `Cargo.toml`）即剪枝不再深入，消除误报；随后在 4 线程池（macOS 下 `setiopolicy_np` 降 I/O 优先级，全仓唯一 `unsafe`）里**并行计算目录大小并逐个流式 emit `Found`**，避免大目录静默上百秒。

新增/修改规则时：`rules.rs` 的测试是行为契约（安全分级 rubric、下载缓存必须 Safe、`dir_name` 规则必配守卫、`.gradle` 必须窄化、不得引用用户数据路径等），改规则先看它们。

### 安全模型（务必先读 `CONCEPTS.md` 与 `models.rs` 的 `SafetyLevel` 文档注释）

`SafetyLevel` 是**两条判据串联的决策树，不是单轴**：① 会丢不可再生数据/状态 → `Risky`；② 否则看重建摩擦 → 需用户主动重建 → `Moderate`，自动透明补回 → `Safe`。**预选与等级解耦**：`selected = safety != Risky && rule.preselect`（`ScanItem::new` / `with_preselect`），CLI `--yes` 与 TUI 默认勾选共用此语义。**非对称陷阱**：CLI 的 `--yes` 与交互确认都基于 `selected`（`selected_items()`），**永不选中 Risky**——Risky 项仅可经 TUI 的 **type-to-confirm** 删除（输入 `CONFIRM_TOKEN` = `"delete"`，Enter 不绑定确认）。删除默认 **移废纸篓**（`DeleteMode::Trash`，可恢复）；CLI `--permanent` 例外（永久、不可恢复），TUI 无永久删除路径。

### TUI 状态机（`tui/src/app.rs` `AppState` + `lib.rs`）

`Menu → Scanning → Results → Cleaning → Done`，外加 `AnalyzingLive`（增量建树中可导航）→ `Sorting`（后台 finalize）→ `Analyzing`（纯内存导航）。`lib.rs` 是主循环 + 全部按键/事件分发（较大）；`ui/*.rs` 按状态拆分渲染。几个反复出现的约束：

- **统一标记集 `App.marked`（`HashSet<PathBuf>`）** 是 Results 与 Analyzer 共用的"待删路径"单一来源；所有删除只作用于它。
- **显示序 vs 存储序**：增量树的 `children` 永远是发现顺序，实时按体积降序展示靠 `size_desc_order` 产出的**索引置换**表达；`cursor` 是显示坐标，`nav_path`/`depth_stack` 是存储坐标，跨系必经置换翻译且一律走 `.get()`（把流式重排的 TOCTOU 竞态降为 no-op）。详见 `docs/solutions/design-patterns/render-layer-sort-permutation-indices.md`。
- 扫描/建树进行中**禁止按位置标记/删除**（列表在实时重排，会误标当下最大项）。
- 派生优先于冗余存储：已发现项数/总大小直接从 `scan_result` 派生，不单独维护。

## 开发流程（Compound Engineering）

本仓库用 [compound-engineering 插件](https://github.com/EveryInc/compound-engineering-plugin) 的循环开发，`docs/` 各目录即各阶段产物。每步是一个 slash 命令（用法见各 skill，此处只给整体流水线与落点）：

| 阶段 | 命令 | 产物 |
| --- | --- | --- |
| 构思 | `/ce-brainstorm` | `docs/brainstorms/`（需求）·`docs/ideation/`（点子） |
| 计划 | `/ce-plan` | `docs/plans/`（按 `YYYY-MM-DD-NNN-<type>-<slug>-plan.md` 命名） |
| 实现 | `/ce-work` | 代码（在分支/worktree，非 `main`） |
| 精简 | `/ce-simplify-code` | —（无 docs 产物） |
| 评审 | `/ce-code-review` | —（无 docs 产物） |
| 提交 | `/ce-commit-push-pr` | commit + PR |
| 沉淀 | `/ce-compound` | `docs/solutions/`（可复用学习）·`CONCEPTS.md`（领域词汇） |

方向层由 `STRATEGY.md`（`/ce-strategy`）承载，计划阶段会读它。要点：改动前先查 `docs/solutions/` 有无相关学习；解决新问题后回到 `/ce-compound` 沉淀，让下一轮更省力。

## 约定

- **语言：全中文**——注释、文档、提交信息、Todo。代码注释密度高且解释"为什么"（常引用具体 bug/审查条目如 D4/R9/KTD8），沿用这种风格。
- **Lint：pedantic 全开 + 渐进式 allow**（根 `Cargo.toml` `[workspace.lints]`，各 crate `[lints] workspace = true`）。`unsafe_code = "deny"`（仅 `scanner.rs` 的 `setiopolicy_np` 一处 `#[allow]` 例外）。新 clippy pedantic lint 随工具链自动生效——不要为绕过而加 `#[allow]`，先修。背景见 `docs/solutions/tooling-decisions/rust-workspace-pedantic-clippy-and-release-profile.md`。
- **工作流 hook**（`.claude/settings.json` + `.claude/hooks/*.sh`，已入库随 worktree 走）：`main` 分支上禁止 Edit/Write；每次编辑 `.rs` 后自动对所属 crate 跑 clippy。故不在 `main` 上直接改代码——用分支/worktree。
- **文档目录**：`docs/brainstorms/`（需求）、`docs/ideation/`、`docs/plans/`、`docs/solutions/`（可复用学习，带 frontmatter；实现前值得先查）。已解决的问题沉淀到 `docs/solutions/`，领域词汇沉淀到 `CONCEPTS.md`。
- 错误处理：核心用 `anyhow`；清理逐项**优雅降级**（单项失败记录后继续），`CleaningDone.deleted_paths` 只含**成功**删除项（TUI 剪树的唯一安全数据源，勿把失败项混入）。
