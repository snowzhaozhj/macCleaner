//! 在 Finder 中显示并选中某路径（move 6 审查面孔 / R5 R8）。
//!
//! 展开态「审查面孔」的只读出口：开发者点开在 Finder 中定位实际文件核对，再决定删不删。
//! **只读揭示**——不删除、不移动、不改文件（R8）。macOS `open -R <path>` 是原生的
//! reveal-and-select（区别于 `open <path>` 会打开文件本身）；`tauri_plugin_opener` 无
//! 对应 free 函数（`reveal_item_in_dir` 仅为 `Opener` trait 方法），故直接 fork 系统 `open`，
//! 与 `open_trash` 同为「瞬时 fork、无 unsafe」的加性小命令。

use std::path::Path;
use std::process::Command;

/// 在 Finder 中定位并选中 `path`。
/// 同步命令：仅 fork 系统 `open -R` 进程、瞬时返回，无需异步（避免 `clippy::unused_async`）。
#[tauri::command]
pub fn reveal_in_finder(path: &str) -> Result<(), String> {
    // 不存在的路径直接报错而非静默——审查面孔要诚实（路径可能刚被删/移）。
    if !Path::new(path).exists() {
        return Err(format!("路径不存在: {path}"));
    }
    let status = Command::new("open")
        .arg("-R")
        .arg(path)
        .status()
        .map_err(|e| format!("在 Finder 中显示失败: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("在 Finder 中显示失败（open -R 退出码 {status}）"))
    }
}
