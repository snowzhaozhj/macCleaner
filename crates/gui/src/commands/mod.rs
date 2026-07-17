//! Tauri 命令层：每个子模块封装一条产品能力，全部经 `mc_core::engine::Engine`。

pub mod analyze;
pub mod clean;
pub mod permission;
pub mod purge;
pub mod reveal;
pub mod trash;
pub mod undo;
pub mod uninstall;

use mc_core::history::{self, HistoryCommand};
use mc_core::models::{CleanReport, SafetyLevel, ScanItem};
use serde::Serialize;

/// type-to-confirm 口令（与前端 `confirm.ts` 的 `CONFIRM_TOKEN` 一致）。
pub(crate) const CONFIRM_TOKEN: &str = "delete";

/// 校验确认口令（trim + 大小写不敏感，对齐前端 `isConfirmed`）。
/// Clean 与 Analyze 两条删除路径共用，避免口令语义在两处漂移。
pub(crate) fn is_confirmed(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case(CONFIRM_TOKEN)
}

/// 删除授权闸（Clean 与 Purge 共用，防校验语义在两处漂移）：
/// 选中项含 `Risky` 时必须携带有效确认口令，否则拒删（防前端 bug/直连 IPC 绕过 type-to-confirm）。
pub(crate) fn authorize_deletion(items: &[ScanItem], confirm_token: &str) -> Result<(), String> {
    if items.iter().any(|i| i.safety == SafetyLevel::Risky) && !is_confirmed(confirm_token) {
        return Err("含危险项，需输入确认口令方可删除".to_string());
    }
    Ok(())
}

/// clean/purge 命令的响应：清理报告 + 本次账本条目的 `run_id`（供回执一键撤销精确命中）。
///
/// `run_id` 为 `None` 表示"无可撤销目标"：无成功项（不写账本）或写账本失败（旁路降级）。
/// 前端据此不显示撤销按钮，退回既有"在访达中恢复"手动路径。
#[derive(Debug, Clone, Serialize)]
pub struct CleanResponse {
    pub report: CleanReport,
    pub run_id: Option<String>,
}

/// clean/purge 成功后写清理账本，返回本次条目的 `run_id`（供回执撤销）。
///
/// **旁路观测语义**（与 CLI `commands/history.rs::record` 一致）：
/// - 无成功项 → 不写账本、返回 `None`（避免空记录污染账本）。
/// - 写失败 → 只 `log::warn!`、返回 `None`，**绝不**改变清理结果或让命令失败。
///
/// 成功写入才返回 `Some(run_id)`——只有此时才存在可确定性撤销的账本条目。
pub(crate) fn record_history(
    command: HistoryCommand,
    items: &[&ScanItem],
    report: &CleanReport,
) -> Option<String> {
    if report.success_count == 0 {
        return None;
    }
    let entry = history::HistoryEntry::from_report(command, items, report);
    let path = history::default_path();
    match history::record(&entry, &path) {
        Ok(()) => Some(entry.run_id),
        Err(e) => {
            log::warn!("写入清理账本失败（已忽略，不影响清理结果）: {e:?}");
            None
        }
    }
}
