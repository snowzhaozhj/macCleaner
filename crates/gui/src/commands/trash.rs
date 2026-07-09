//! 打开系统废纸篓（U5 / R11 / KTD4 路 B）。
//!
//! 「在访达中恢复」的唯一后端触点：在 Finder 中打开 `~/.Trash`，用户借 Finder 原生
//! 「放回原处」恢复文件。**不做延迟执行的假 undo**——真一键 undo 是 macOS 上的非平凡
//! 独立命题（`trash` crate 不支持 restore、Trash 非事务日志），单独立项，见计划 Scope Boundaries。

use tauri_plugin_opener::open_path;

/// 在 Finder 中打开 `~/.Trash`。复用 `mc_core` 的 home 解析，路径口径与清理/移废纸篓一致。
/// 同步命令：仅 fork 系统 `open` 进程、瞬时返回，无需异步（避免 `clippy::unused_async`）。
#[tauri::command]
pub fn open_trash() -> Result<(), String> {
    let trash = mc_core::platform::get_home_dir().join(".Trash");
    open_path(&trash, None::<&str>).map_err(|e| format!("打开废纸篓失败: {e}"))
}
