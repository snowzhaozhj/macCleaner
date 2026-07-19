pub mod analyze;
pub mod clean;
pub mod doctor;
pub mod history;
pub mod orphans;
pub mod purge;
pub mod uninstall;
pub mod undo;

/// 用户叠加规则加载提示（#2 规则外部化）：成功加载 ≥1 条时在 stderr 告知本次扫描含用户规则。
/// clean 与 purge 共用。文件不存在/门禁不过 → `user_rules()` 返回空 → 静默（零噪音）。
/// 走 stderr 保证 `--json` 的 stdout 流不被污染。
pub fn print_user_rules_notice() {
    let count = mc_core::rules::user_rules().len();
    if count > 0 {
        eprintln!("已加载 {count} 条用户叠加规则（~/.config/mc/rules.toml）\n");
    }
}
