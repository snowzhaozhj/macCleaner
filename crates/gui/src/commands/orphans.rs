//! Orphans 命令：反向卸载——扫 `~/Library` 找父 App 已不存在的孤儿残留、移废纸篓删除。
//! 与 `mc uninstall`（正向：选一个已装 App 卸载它的残留）互补；镜像 CLI `mc orphans`。
//!
//! 扫描端走同步 `Engine::scan_orphans`（非流式、无取消），删除端复用 clean/purge 的
//! 信任链：取项 `select_by_paths`、授权闸 `authorize_deletion`、回执 `CleanResponse` +
//! 写账本，不复制逻辑（KTD1）。**孤儿一律不预选**由核心 `scan_orphans` 保证（KTD2）——
//! 工具主动发现、非用户点名，故永不默认删、永不自动删，须用户逐项手动勾。

use std::collections::HashSet;
use std::path::PathBuf;

use mc_core::engine::Engine;
use mc_core::history::{self, HistoryCommand};
use mc_core::models::{CategoryGroup, DeleteMode, ScanItem, ScanResult};
use mc_core::progress::ProgressEvent;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::{authorize_deletion, clean::select_by_paths, CleanResponse};
use crate::reporter::TauriReporter;
use crate::AppState;

/// 同步扫描孤儿残留，合成单组 `ScanResult` 存入 `last_orphans` 槽、回传前端渲染。
///
/// 走同步 `Engine::scan_orphans` 而非流式：核心一次性返回全量快照、不 emit 事件、无中途
/// 可取消的增量流，故不装 `begin_operation` 取消 flag、不引进度 Channel（KTD1，同
/// `scan_uninstall`）。`scan_orphans` 内部 `list_apps` + 遍历 `~/Library` 可能数秒，放
/// `spawn_blocking` 避免冻结 async 运行时线程。**孤儿一律 `preselect=false`** 由核心保证，
/// 前端 `selected` 直接映射即天然全未勾（KTD2）。
///
/// 单组收纳（同 uninstall `resolve_leftovers`）：前端按各项自身 `category`（如
/// 「应用残留 (Caches)」）重新分组渲染，故后端分组结构不影响展示，无需在此按子目录再拆组。
#[tauri::command]
pub async fn scan_orphans(app: AppHandle) -> Result<ScanResult, String> {
    let last_orphans = app.state::<AppState>().last_orphans.clone();
    let ticket = last_orphans.begin();
    let result = tauri::async_runtime::spawn_blocking(|| {
        let items = Engine::scan_orphans();
        ScanResult::from_categories(vec![CategoryGroup::new("孤儿残留".to_string(), items)])
    })
    .await
    .map_err(|e| format!("扫描孤儿残留线程异常: {e}"))?;
    // 代次守卫写槽：乱序完成时旧扫描不覆盖新结果（见 slot.rs）。
    last_orphans.commit(ticket, result.clone())?;
    Ok(result)
}

/// 移废纸篓删除给定路径集（恒用 `DeleteMode::Trash`——GUI 无永久删除路径，R4）。
/// 待删项从 `last_orphans` 按路径精确取出（R4，不接受前端回传完整 `ScanItem`）。
///
/// `confirm_token`：**后端二次校验**——含 `Risky` 项须携带有效确认口令方可删除。核心保证
/// 孤儿分级只到 Moderate + Safe、永不 Risky，故此闸对孤儿的真实批次恒放行；仍保留是纵深
/// 防御，防前端 bug/直连 IPC 注入 Risky 项绕过（KTD5，与 clean/purge/uninstall 三处删除闸一致）。
/// 删除成功写账本、回传 `run_id` 供回执一键撤销（KTD4：复用 `HistoryCommand::Clean`——
/// 撤销按 `run_id` + inode 确定性放回，与命令类型无关；孤儿删除本质即「移废纸篓」）。
#[tauri::command]
pub async fn clean_orphans(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanResponse, String> {
    let (cancelled, last_orphans) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_orphans.clone())
    };
    let selected: HashSet<PathBuf> = paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 短临界区 clone 出 owned 待删项后立即 drop 锁——与 clean/purge 同理：
        // 避免删除全程持锁，Engine::clean panic 会毒化 last_orphans 卡死后续命令。
        let items: Vec<ScanItem> = {
            let guard = last_orphans.read()?;
            let scan = guard.1.as_ref().ok_or_else(|| "无孤儿扫描结果可清理".to_string())?;
            select_by_paths(scan, &selected).into_iter().cloned().collect()
        };
        authorize_deletion(&items, &confirm_token)?;
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        let report =
            Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("清理失败: {e}"))?;
        // 旁路写账本 + 回传 run_id（无成功项/写失败 → None，前端不显撤销）。
        let run_id = history::record_run(HistoryCommand::Clean, &refs, &report);
        Ok(CleanResponse { report, run_id })
    })
    .await
    .map_err(|e| format!("清理线程异常: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::models::SafetyLevel;

    fn item(path: &str, safety: SafetyLevel) -> ScanItem {
        ScanItem::new(PathBuf::from(path), 100, safety, "孤儿残留".into())
    }

    fn scan_with(items: Vec<ScanItem>) -> ScanResult {
        ScanResult::from_categories(vec![CategoryGroup::new("孤儿残留".into(), items)])
    }

    /// R7：orphans 删除只从 orphans 槽取项——clean/purge 结果里的路径在 orphans 槽中不命中。
    #[test]
    fn orphans_selection_is_isolated_from_other_slots() {
        let orphans_scan = scan_with(vec![item("/o/com.foo.app", SafetyLevel::Safe)]);
        let other_paths: HashSet<PathBuf> = ["/c/cache"].iter().map(PathBuf::from).collect();
        assert!(
            select_by_paths(&orphans_scan, &other_paths).is_empty(),
            "其他槽的路径不得从 orphans 槽命中（隔离）"
        );
        // 对照：同路径在自身槽正常命中，证明未命中是隔离而非取项失效。
        let own: HashSet<PathBuf> = ["/o/com.foo.app"].iter().map(PathBuf::from).collect();
        assert_eq!(
            select_by_paths(&orphans_scan, &own).len(),
            1,
            "orphans 槽内自身路径应正常命中"
        );
    }

    /// R4/KTD5：含 Risky 且口令无效 → 拒删；口令有效（trim/大小写不敏感）→ 放行。
    #[test]
    fn risky_items_require_valid_confirm_token() {
        let risky = vec![item("/o/com.foo.app", SafetyLevel::Risky)];
        assert!(authorize_deletion(&risky, "").is_err());
        assert!(authorize_deletion(&risky, "del").is_err());
        assert!(authorize_deletion(&risky, "delete").is_ok());
        assert!(authorize_deletion(&risky, "  DELETE\n").is_ok());
    }

    /// 孤儿的真实分级面（Safe/Moderate）对口令无要求——KTD5 闸恒放行。
    #[test]
    fn non_risky_orphans_need_no_token() {
        let items = vec![
            item("/o/com.a.app", SafetyLevel::Safe),
            item("/o/com.b.app", SafetyLevel::Moderate),
        ];
        assert!(authorize_deletion(&items, "").is_ok());
    }

    #[test]
    fn empty_or_unknown_paths_select_nothing() {
        let scan = scan_with(vec![item("/o/com.foo.app", SafetyLevel::Safe)]);
        assert!(select_by_paths(&scan, &HashSet::new()).is_empty());
        let unknown: HashSet<PathBuf> = ["/does-not-exist"].iter().map(PathBuf::from).collect();
        assert!(select_by_paths(&scan, &unknown).is_empty());
    }
}
