//! Purge 命令：流式扫描指定目录下的开发产物（`node_modules`/`target`/`DerivedData`…）、
//! 移废纸篓删除、协作式取消。全部经 `mc_core::engine::Engine`；取项复用
//! `clean::select_by_paths`、授权闸复用 `commands::authorize_deletion`，不复制逻辑（KTD1）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mc_core::engine::Engine;
use mc_core::models::{CleanReport, DeleteMode, ScanItem, ScanResult};
use mc_core::progress::ProgressEvent;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::{authorize_deletion, clean::select_by_paths};
use crate::reporter::TauriReporter;
use crate::AppState;

/// 流式扫描 `path` 下的开发产物。进度经 `on_event` Channel 实时推前端；
/// 结果存入独立的 `last_purge` 槽（KTD2：与 clean 的 `last_scan` 隔离，
/// 切 tab 或交替扫描时删除不会误取另一路径的项，R8/AE5）。
#[tauri::command]
pub async fn scan_purge(
    app: AppHandle,
    path: String,
    on_event: Channel<ProgressEvent>,
) -> Result<ScanResult, String> {
    // 在 await 前取出 owned 句柄，避免 async 命令持有 State<'_,_> 借用（KTD-5）。
    let (cancelled, last_purge) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_purge.clone())
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::scan_purge(Path::new(&path), &reporter)
    })
    .await
    .map_err(|e| format!("扫描线程异常: {e}"))?
    .map_err(|e| format!("扫描失败: {e}"))?;
    *last_purge.lock().map_err(|_| "状态锁毒化".to_string())? = Some(result.clone());
    Ok(result)
}

/// 移废纸篓删除给定路径集（恒用 `DeleteMode::Trash`——GUI 无永久删除路径，R10）。
/// 待删项从 `last_purge` 按路径精确取出（R11，不接受前端回传完整 `ScanItem`）；
/// 含 Risky 时后端二次校验确认口令（复用 `authorize_deletion`），防前端绕过。
#[tauri::command]
pub async fn purge(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanReport, String> {
    let (cancelled, last_purge) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_purge.clone())
    };
    let selected: HashSet<PathBuf> = paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 短临界区 clone 出 owned 待删项后立即 drop 锁——与 clean 同理：
        // 避免删除全程持锁，Engine::clean panic 会毒化 last_purge 卡死后续命令。
        let items: Vec<ScanItem> = {
            let guard = last_purge.lock().map_err(|_| "状态锁毒化".to_string())?;
            let scan = guard.as_ref().ok_or_else(|| "无扫描结果可清理".to_string())?;
            select_by_paths(scan, &selected).into_iter().cloned().collect()
        };
        authorize_deletion(&items, &confirm_token)?;
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("清理失败: {e}"))
    })
    .await
    .map_err(|e| format!("清理线程异常: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::models::{CategoryGroup, SafetyLevel};

    fn item(path: &str, safety: SafetyLevel) -> ScanItem {
        ScanItem::new(PathBuf::from(path), 100, safety, "开发产物".into())
    }

    fn scan_with(items: Vec<ScanItem>) -> ScanResult {
        ScanResult::from_categories(vec![CategoryGroup::new("开发产物".into(), items)])
    }

    /// AE5：purge 删除只从 purge 扫描结果取项——clean 结果里的路径在 purge 槽中不命中。
    #[test]
    fn purge_selection_is_isolated_from_clean_result() {
        let purge_scan = scan_with(vec![item("/p/node_modules", SafetyLevel::Safe)]);
        let clean_scan = scan_with(vec![item("/c/cache", SafetyLevel::Safe)]);
        let clean_paths: HashSet<PathBuf> = ["/c/cache"].iter().map(PathBuf::from).collect();
        assert!(
            select_by_paths(&purge_scan, &clean_paths).is_empty(),
            "clean 结果的路径不得从 purge 槽命中（隔离）"
        );
        assert_eq!(
            select_by_paths(&clean_scan, &clean_paths).len(),
            1,
            "同一路径在 clean 槽应正常命中——证明未命中是隔离而非取项失效"
        );
    }

    /// R10：含 Risky 且口令无效 → 拒删；口令有效（trim/大小写不敏感）→ 放行。
    #[test]
    fn risky_items_require_valid_confirm_token() {
        let risky = vec![item("/p/.gradle", SafetyLevel::Risky)];
        assert!(authorize_deletion(&risky, "").is_err());
        assert!(authorize_deletion(&risky, "del").is_err());
        assert!(authorize_deletion(&risky, "delete").is_ok());
        assert!(authorize_deletion(&risky, "  DELETE\n").is_ok());
    }

    /// 纯 Safe/Moderate 批次对口令无要求（AE7 的后端面）。
    #[test]
    fn non_risky_items_need_no_token() {
        let items = vec![
            item("/p/node_modules", SafetyLevel::Safe),
            item("/p/target", SafetyLevel::Moderate),
        ];
        assert!(authorize_deletion(&items, "").is_ok());
    }

    #[test]
    fn empty_or_unknown_paths_select_nothing() {
        let scan = scan_with(vec![item("/p/node_modules", SafetyLevel::Safe)]);
        assert!(select_by_paths(&scan, &HashSet::new()).is_empty());
        let unknown: HashSet<PathBuf> = ["/does-not-exist"].iter().map(PathBuf::from).collect();
        assert!(select_by_paths(&scan, &unknown).is_empty());
    }
}
