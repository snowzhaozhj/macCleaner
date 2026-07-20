//! Clean 命令：流式扫描系统/浏览器缓存、移废纸篓删除、协作式取消。
//! 全部经 `mc_core::engine::Engine`，不复制任何扫描/清理逻辑（R1）。

use std::collections::HashSet;
use std::path::PathBuf;

use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, ScanItem, ScanResult};
use mc_core::progress::ProgressEvent;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::{authorize_deletion, CleanResponse};
use crate::reporter::TauriReporter;
use crate::AppState;
use mc_core::history::{self, HistoryCommand};

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
    // slot.begin() 领代次，须在 spawn_blocking 前调用，代次才反映本命令发起次序。
    let (cancelled, last_scan) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_scan.clone())
    };
    let ticket = last_scan.begin();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::scan_clean(&reporter)
    })
    .await
    .map_err(|e| format!("扫描线程异常: {e}"))?
    .map_err(|e| format!("扫描失败: {e}"))?;
    // 代次守卫写槽：乱序完成时旧扫描不覆盖新结果（见 slot.rs）。commit 返回 false 表示
    // 被更新的扫描抢先——本次仍回传前端渲染，槽保留最新者，删除永远按最新槽。
    last_scan.commit(ticket, result.clone())?;
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
) -> Result<CleanResponse, String> {
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
            let guard = last_scan.read()?;
            let scan = guard.1.as_ref().ok_or_else(|| "无扫描结果可清理".to_string())?;
            select_by_paths(scan, &selected).into_iter().cloned().collect()
        };
        // 后端闸：含 Risky 必须有有效确认口令，否则拒删（与 purge 共用 authorize_deletion）。
        authorize_deletion(&items, &confirm_token)?;
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        let report =
            Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("清理失败: {e}"))?;
        // 旁路写账本 + 回传 run_id 供回执一键撤销（无成功项/写失败 → None，前端不显撤销）。
        let run_id = history::record_run(HistoryCommand::Clean, &refs, &report);
        Ok(CleanResponse { report, run_id })
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
    use crate::commands::is_confirmed;
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
