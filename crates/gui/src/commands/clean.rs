//! Clean 命令：流式扫描系统/浏览器缓存、移废纸篓删除、协作式取消。
//! 全部经 `mc_core::engine::Engine`，不复制任何扫描/清理逻辑（R1）。

use std::collections::HashSet;
use std::path::PathBuf;

use mc_core::engine::Engine;
use mc_core::models::{CleanReport, DeleteMode, SafetyLevel, ScanItem, ScanResult};
use mc_core::progress::ProgressEvent;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::reporter::TauriReporter;
use crate::AppState;

/// type-to-confirm 口令（与前端 `confirm.ts` 的 `CONFIRM_TOKEN` 一致）。
const CONFIRM_TOKEN: &str = "delete";

/// 校验确认口令（trim + 大小写不敏感，对齐前端 `isConfirmed`）。
fn is_confirmed(token: &str) -> bool {
    token.trim().eq_ignore_ascii_case(CONFIRM_TOKEN)
}

/// 按路径集从扫描结果中挑出待删项（纯函数，便于单测）。
/// 前端传来用户选中/标记的路径；Risky 项须经 type-to-confirm（U8）后才会出现在此集合中。
pub fn select_by_paths<'a, S: std::hash::BuildHasher>(
    scan: &'a ScanResult,
    paths: &HashSet<PathBuf, S>,
) -> Vec<&'a ScanItem> {
    scan.categories
        .iter()
        .flat_map(|c| c.items.iter())
        .filter(|i| paths.contains(&i.path))
        .collect()
}

/// 流式扫描 clean 分类。进度经 `on_event` Channel 实时推前端；
/// 结果存入 `last_scan` 供后续 `clean` 精确取项，同时回传前端渲染列表。
#[tauri::command]
pub async fn scan_clean(
    app: AppHandle,
    on_event: Channel<ProgressEvent>,
) -> Result<ScanResult, String> {
    // 在 await 前取出 owned 句柄，避免 async 命令持有 State<'_,_> 借用（KTD-5）。
    // begin_operation 安装本次操作专属的取消 flag（R-review：不再复位共享 flag）。
    let (cancelled, last_scan) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_scan.clone())
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::scan_clean(&reporter)
    })
    .await
    .map_err(|e| format!("扫描线程异常: {e}"))?
    .map_err(|e| format!("扫描失败: {e}"))?;
    *last_scan.lock().map_err(|_| "状态锁毒化".to_string())? = Some(result.clone());
    Ok(result)
}

/// 移废纸篓删除给定路径集（恒用 `DeleteMode::Trash`——GUI 无永久删除路径，R7/AE3）。
/// 待删项从上次扫描结果中按路径精确取出，避免前端回传完整 `ScanItem`。
///
/// `confirm_token`：**后端二次校验**——若选中项含 `Risky`，须携带有效确认口令
/// （trim + 大小写不敏感 == `"delete"`）方可删除。前端 type-to-confirm 之外再加此闸，
/// 防前端 bug/直连 IPC 绕过（R-review codex-P1）。纯非 Risky 删除对口令无要求。
#[tauri::command]
pub async fn clean(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanReport, String> {
    let (cancelled, last_scan) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_scan.clone())
    };
    let selected: HashSet<PathBuf> = paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 先在持锁的短临界区内 clone 出 owned 待删项，随即 drop 锁——
        // 避免删除全程持锁：一旦 Engine::clean 内部 panic 会毒化 last_scan，
        // 永久使后续 scan_clean/clean 报「状态锁毒化」（R-review）。
        let items: Vec<ScanItem> = {
            let guard = last_scan.lock().map_err(|_| "状态锁毒化".to_string())?;
            let scan = guard.as_ref().ok_or_else(|| "无扫描结果可清理".to_string())?;
            select_by_paths(scan, &selected).into_iter().cloned().collect()
        };
        // 后端闸：含 Risky 必须有有效确认口令，否则拒删（防绕过前端 type-to-confirm）。
        if items.iter().any(|i| i.safety == SafetyLevel::Risky) && !is_confirmed(&confirm_token) {
            return Err("含危险项，需输入确认口令方可删除".to_string());
        }
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("清理失败: {e}"))
    })
    .await
    .map_err(|e| format!("清理线程异常: {e}"))?
}

/// 触发协作式取消（对**当前**操作的 flag 置位，瞬时非阻塞）。声明为 async 使 `AppHandle`
/// move 进 future，与 `scan_clean`/`clean` 一致，且不占主线程（KTD-5）。
#[tauri::command]
pub async fn cancel_scan(app: AppHandle) {
    app.state::<AppState>().request_cancel();
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::models::{CategoryGroup, SafetyLevel};

    fn item(path: &str, safety: SafetyLevel) -> ScanItem {
        ScanItem::new(PathBuf::from(path), 100, safety, "缓存".into())
    }

    fn scan_with(items: Vec<ScanItem>) -> ScanResult {
        ScanResult::from_categories(vec![CategoryGroup::new("缓存".into(), items)])
    }

    #[test]
    fn selects_only_requested_paths() {
        let scan = scan_with(vec![
            item("/a", SafetyLevel::Safe),
            item("/b", SafetyLevel::Moderate),
            item("/c", SafetyLevel::Risky),
        ]);
        let paths: HashSet<PathBuf> = ["/a", "/c"].iter().map(PathBuf::from).collect();
        let picked = select_by_paths(&scan, &paths);
        let picked_paths: HashSet<PathBuf> = picked.iter().map(|i| i.path.clone()).collect();
        assert_eq!(picked_paths, paths, "只应选中请求的路径（含被显式确认的 Risky /c）");
    }

    #[test]
    fn empty_selection_yields_nothing() {
        let scan = scan_with(vec![item("/a", SafetyLevel::Safe)]);
        assert!(select_by_paths(&scan, &HashSet::new()).is_empty());
    }

    #[test]
    fn unknown_paths_are_ignored() {
        let scan = scan_with(vec![item("/a", SafetyLevel::Safe)]);
        let paths: HashSet<PathBuf> = ["/does-not-exist"].iter().map(PathBuf::from).collect();
        assert!(select_by_paths(&scan, &paths).is_empty());
    }

    #[test]
    fn confirm_token_trims_and_ignores_case() {
        assert!(is_confirmed("delete"));
        assert!(is_confirmed("DELETE"));
        assert!(is_confirmed("  Delete\n"));
        assert!(!is_confirmed(""));
        assert!(!is_confirmed("del"));
        assert!(!is_confirmed("delete please"));
    }
}
