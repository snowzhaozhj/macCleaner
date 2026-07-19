//! Tauri 命令层：每个子模块封装一条产品能力，全部经 `mc_core::engine::Engine`。

pub mod analyze;
pub mod clean;
pub mod orphans;
pub mod permission;
pub mod purge;
pub mod reveal;
pub mod trash;
pub mod undo;
pub mod uninstall;

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
