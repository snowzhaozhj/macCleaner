//! macCleaner GUI（Tauri 后端）。进程内直接复用 `mc-core` 引擎，
//! 前端经 `ipc::Channel` 收流式进度、经 `invoke` 触发命令与取消。
//!
//! 分层见 `docs/plans/2026-07-07-014-feat-gui-mvp-scope-plan.md`：
//! commands（命令层）· reporter（`ProgressReporter` → `ipc::Channel` 适配）·
//! 取消用 managed `Arc<AtomicBool>`（KTD-5）。

pub mod commands;
pub mod reporter;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use mc_core::models::{DirNode, ScanResult};

/// 应用共享状态（Tauri managed state）。
/// 命令进入阻塞闭包前克隆其中的 `Arc`（KTD-5：async 命令不可持有 `State<'_,_>` 借用）。
pub struct AppState {
    /// **当前操作**的取消标志。每条命令入口经 `begin_operation` 安装一个全新 flag：
    /// 旧操作仍持有自己那份 `Arc`，不会被后续命令「反取消」（R-review：修共享 flag 反取消）。
    /// `cancel_scan` 只对**当前** flag 置位；各操作的 reporter 持有各自 flag 读 `is_cancelled`。
    pub cancel: Mutex<Arc<AtomicBool>>,
    /// 最近一次 clean 扫描结果，供 `clean` 按路径精确取项（避免前端回传完整 `ScanItem`）。
    pub last_scan: Arc<Mutex<Option<ScanResult>>>,
    /// 最近一次 analyze 树，供 `delete_marked` 按标记路径收集 (path, size)。
    pub last_analyze: Arc<Mutex<Option<DirNode>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            last_scan: Arc::new(Mutex::new(None)),
            last_analyze: Arc::new(Mutex::new(None)),
        }
    }
}

impl AppState {
    /// 为一次新操作安装并返回全新取消 flag。旧 flag 被替换但旧操作仍持有其 `Arc`，
    /// 故后续命令启动不会「反取消」仍在收尾的旧操作。锁毒化无害（仅守护一个 `Arc`），恢复即用。
    pub fn begin_operation(&self) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap_or_else(PoisonError::into_inner) = flag.clone();
        flag
    }

    /// 对**当前**操作的取消 flag 置位（`cancel_scan` 用）。
    pub fn request_cancel(&self) {
        self.cancel
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .store(true, Ordering::Relaxed);
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
            commands::permission::check_fda,
            commands::permission::open_fda_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
