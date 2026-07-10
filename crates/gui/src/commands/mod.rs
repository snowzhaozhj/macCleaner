//! Tauri 命令层：每个子模块封装一条产品能力，全部经 `mc_core::engine::Engine`。

pub mod analyze;
pub mod clean;
pub mod permission;
pub mod reveal;
pub mod trash;

/// type-to-confirm 口令（与前端 `confirm.ts` 的 `CONFIRM_TOKEN` 一致）。
pub(crate) const CONFIRM_TOKEN: &str = "delete";

/// 校验确认口令（trim + 大小写不敏感，对齐前端 `isConfirmed`）。
/// Clean 与 Analyze 两条删除路径共用，避免口令语义在两处漂移。
pub(crate) fn is_confirmed(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case(CONFIRM_TOKEN)
}
