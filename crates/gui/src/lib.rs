//! macCleaner GUI（Tauri 后端）。进程内直接复用 `mc-core` 引擎，
//! 前端经 `ipc::Channel` 收流式进度、经 `invoke` 触发命令与取消。
//!
//! 分层见 `docs/plans/2026-07-07-014-feat-gui-mvp-scope-plan.md`：
//! commands（命令层）· reporter（`ProgressReporter` → `ipc::Channel` 适配）·
//! 取消用 managed `Arc<AtomicBool>`（KTD-5）。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// 协作式取消的共享标志，放进 Tauri managed state。
/// 扫描/分析命令进入阻塞闭包前克隆出其中的 `Arc`（KTD-5：async 命令不可持有 `State<'_,_>` 借用）。
pub struct ScanCancel(pub Arc<AtomicBool>);

impl Default for ScanCancel {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(ScanCancel::default())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 占位健康检查命令（U2 脚手架）。后续单元补 `scan_clean` / `clean` / `analyze` / FDA 命令。
#[tauri::command]
fn ping() -> &'static str {
    "pong"
}
