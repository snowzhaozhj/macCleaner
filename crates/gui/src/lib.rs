//! macCleaner GUI（Tauri 后端）。进程内直接复用 `mc-core` 引擎，
//! 前端经 `ipc::Channel` 收流式进度、经 `invoke` 触发命令与取消。
//!
//! 分层见 `docs/plans/2026-07-07-014-feat-gui-mvp-scope-plan.md`：
//! commands（命令层）· reporter（`ProgressReporter` → `ipc::Channel` 适配）·
//! 取消用 managed `Arc<AtomicBool>`（KTD-5）。

pub mod commands;
pub mod reporter;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use mc_core::models::{DirNode, ScanResult};

/// 应用共享状态（Tauri managed state）。
/// 命令进入阻塞闭包前克隆其中的 `Arc`（KTD-5：async 命令不可持有 `State<'_,_>` 借用）。
pub struct AppState {
    /// 协作式取消标志：`cancel_scan` 置位，reporter 的 `is_cancelled` 读取。
    pub cancel: Arc<AtomicBool>,
    /// 最近一次 clean 扫描结果，供 `clean` 按路径精确取项（避免前端回传完整 `ScanItem`）。
    pub last_scan: Arc<Mutex<Option<ScanResult>>>,
    /// 最近一次 analyze 树，供 `delete_marked` 按标记路径收集 (path, size)。
    pub last_analyze: Arc<Mutex<Option<DirNode>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            last_scan: Arc::new(Mutex::new(None)),
            last_analyze: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::clean::scan_clean,
            commands::clean::clean,
            commands::clean::cancel_scan,
            commands::analyze::analyze,
            commands::analyze::delete_marked,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
