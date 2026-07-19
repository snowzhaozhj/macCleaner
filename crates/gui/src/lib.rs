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
    /// 最近一次 purge 扫描结果，独立于 `last_scan`（KTD2：clean/purge 隔离，
    /// 切 tab 或交替扫描时删除不会误取另一路径的项）。
    pub last_purge: Arc<Mutex<Option<ScanResult>>>,
    /// 最近一次 uninstall 残留审查结果（app bundle + 残留），独立槽（KTD4：与 clean/purge
    /// 隔离，切 tab 或交替操作时删除不会误取另一路径的项）。
    pub last_uninstall: Arc<Mutex<Option<ScanResult>>>,
    /// 最近一次 analyze 树，供 `delete_marked` 按标记路径收集 (path, size)。
    pub last_analyze: Arc<Mutex<Option<DirNode>>>,
    /// 最近一次 orphans 反向扫描结果，独立槽（KTD3：与 clean/purge/uninstall 隔离，
    /// 切 tab 或交替操作时删除不会误取另一路径的项）。
    pub last_orphans: Arc<Mutex<Option<ScanResult>>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            last_scan: Arc::new(Mutex::new(None)),
            last_purge: Arc::new(Mutex::new(None)),
            last_uninstall: Arc::new(Mutex::new(None)),
            last_analyze: Arc::new(Mutex::new(None)),
            last_orphans: Arc::new(Mutex::new(None)),
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
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::clean::scan_clean,
            commands::clean::clean,
            commands::clean::cancel_scan,
            commands::purge::scan_purge,
            commands::purge::purge,
            commands::uninstall::scan_uninstall,
            commands::uninstall::resolve_leftovers,
            commands::uninstall::uninstall,
            commands::analyze::analyze,
            commands::analyze::classify_marked,
            commands::analyze::delete_marked,
            commands::orphans::scan_orphans,
            commands::orphans::clean_orphans,
            commands::permission::check_fda,
            commands::permission::open_fda_settings,
            commands::trash::open_trash,
            commands::undo::undo,
            commands::reveal::reveal_in_finder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
