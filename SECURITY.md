# 安全边界（Security）

macCleaner（二进制名 `mc`）是一个删除文件的工具，因此它的可信度由**成文的安全边界**与**机器可验证的契约**共同支撑，而非口头承诺。本文说明产品的安全模型；全部清理规则逐条列在 [RULES.md](RULES.md)（由源规则表机械生成）。

## 删除语义：默认可恢复

- **默认移入废纸篓**（`DeleteMode::Trash`）。所有删除动作默认把命中项移入系统废纸篓，可从废纸篓恢复。真正释放磁盘空间需用户手动清空废纸篓——TUI 的 Done 屏会明确复述这一点，避免用户以为工具失效。
- **永久删除是显式例外**。仅 CLI 的 `--permanent` 开关会绕过废纸篓做不可逆删除；TUI **没有**任何永久删除路径。
- **无静默删除**。删除前一律先列出完整待删路径清单再执行（CLI 确认提示 / TUI 确认框）。

## 风险分级与预选：Risky 永不被默认选中

每个可删项按 `SafetyLevel` 分级（判据与 rubric 见 `crates/core/src/models.rs` 的文档注释，领域定义见 `CONCEPTS.md`）：

- **Safe** — 零数据丢失，自动透明补回（共享/下载缓存、IDE 索引）。
- **Moderate** — 零数据丢失，但需用户主动重建一次（`node_modules`、`target`、`DerivedData`）。
- **Risky** — 可能丢失不可再生数据或有价值状态（Docker 命名卷、Xcode Archives 的 dSYM、装好环境的 AVD）。

**预选与等级解耦**：`selected = safety != Risky && rule.preselect`。由此得到两条硬边界：

1. **CLI `--yes` 与 TUI 默认勾选都只作用于已预选项**（`selected_items()`），**永不选中 Risky**。
2. **Risky 项只能经 TUI 的 type-to-confirm 删除**：需手动输入确认令牌 `delete`（`crates/tui/src/lib.rs` 的 `CONFIRM_TOKEN`），Enter 不绑定确认。CLI 无删除 Risky 的路径。

## 路径保护与守卫

- **按目录名匹配的开发产物规则必须配置项目根守卫**（`root_markers`）：例如 `node_modules` 旁需有 `package.json`、`target` 旁需有 `Cargo.toml`、`venv` 内需有 `pyvenv.cfg`，满足才计入，以消除误报。唯一豁免是 `__pycache__`（纯字节码缓存，无误伤风险）。
- **规则不引用用户数据路径**：`Documents`、`Desktop`、`Downloads` 等目录不出现在任何规则里。
- **宽泛通配被刻意窄化**：如 `.gradle` 只精确匹配 `~/.gradle/caches`，绝不整树匹配（否则会误删签名密钥与配置）。

## 隐私与依赖面

- **零遥测**：不采集、不上报任何使用数据，运行时不发起网络请求。
- **规则编译进二进制**：`clean_rules.toml` / `purge_rules.toml` 通过 `include_str!` 编译进 `mc`，无运行时远程规则拉取。
- **全仓仅 1 处 `unsafe`**：`unsafe_code = "deny"` 在 workspace 级强制开启；唯一例外是 `crates/core/src/scanner.rs` 中调用 macOS `setiopolicy_np`（降低扫描 I/O 优先级，避免拖慢前台应用）的一处 `#[allow(unsafe_code)]`。

## 机器可验证的契约

安全边界不靠自觉，由 `crates/core/src/rules.rs` 的单元测试作为**行为契约**在 CI 中持续验证：

- `clean_rules_all_safe`（:191）—— 所有系统缓存规则必须为 Safe。
- `purge_rules_safety_levels`（:203）—— 开发产物按 rubric 分级，可能丢数据/状态的项必须为 Risky。
- `dirname_rules_have_guards`（:353）—— 除 `__pycache__` 外每条按目录名匹配的规则都必须配置项目根守卫。
- `no_rules_reference_user_data_paths`（:421）—— 任何规则都不得引用 `Documents`/`Desktop`/`Downloads`。

此外 CI 有一道 **RULES.md 漂移门禁**：重跑 `cargo run -p xtask -- gen-rules` 后若 `RULES.md` 与提交版本不一致即失败，强制透明度页与规则源始终同步。

## 报告安全问题

如发现可能导致误删用户数据、绕过 Risky 确认、或其它安全隐患的问题，请在本仓库提交 issue（或私下联系维护者）。请附最小复现步骤与受影响的规则名/路径。
