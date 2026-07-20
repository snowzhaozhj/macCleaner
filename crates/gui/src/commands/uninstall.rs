//! Uninstall 命令：两阶段应用卸载——① `scan_uninstall` 列已装应用（同步 `list_apps`，含
//! `bundle_id`）；② `resolve_leftovers` 对选定应用解析 `~/Library` 残留、与 app bundle 合成
//! 一份可审查 `ScanResult` 存入 `last_uninstall`；③ `uninstall` 移废纸篓删。
//!
//! 镜像 CLI `mc uninstall` 的数据流（非 TUI——TUI 从不解析残留）。取项复用
//! `clean::select_by_paths`、授权闸复用 `commands::authorize_deletion`，不复制逻辑（KTD1）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mc_core::engine::Engine;
use mc_core::models::{AppInfo, CategoryGroup, CleanReport, DeleteMode, SafetyLevel, ScanItem, ScanResult};
use mc_core::progress::ProgressEvent;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::{authorize_deletion, clean::select_by_paths};
use crate::reporter::TauriReporter;
use crate::AppState;

/// 阶段一：列出已安装应用（同步 `Engine::list_apps`，含 `bundle_id`）。
///
/// 走同步 `list_apps` 而非流式 `scan_uninstall`：后者的 `Found` 事件丢弃 `bundle_id`，
/// 而阶段二 `resolve_leftovers` 必须要它（KTD1）。`list_apps` 对每个 `.app` 递归
/// 算体积，可能数秒，故放 `spawn_blocking` 避免冻结 async 运行时线程。纯查询、无取消。
#[tauri::command]
pub async fn scan_uninstall() -> Result<Vec<AppInfo>, String> {
    tauri::async_runtime::spawn_blocking(Engine::list_apps)
        .await
        .map_err(|e| format!("扫描应用线程异常: {e}"))
}

/// 阶段二：对选定应用解析残留并与 app bundle 合成一份 `ScanResult`，存入 `last_uninstall`。
///
/// **服务端校验 `app_path`**（R11 信任边界）：`canonicalize` 后须落在 `/Applications` 或
/// `~/Applications` 下且为存在的 `.app`——该路径会成为删除槽里 `Safe` 预选项的删除目标，
/// 不能直接信任前端回传（防直连 IPC 注入任意路径静默移废纸篓）。`app_size` 仅供展示。
/// `find_leftovers` 递归算残留体积，故放 `spawn_blocking`。
#[tauri::command]
pub async fn resolve_leftovers(
    app: AppHandle,
    app_path: String,
    bundle_id: Option<String>,
    app_size: u64,
) -> Result<ScanResult, String> {
    let last_uninstall = app.state::<AppState>().last_uninstall.clone();
    let ticket = last_uninstall.begin();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let canonical = validate_app_path(&app_path)?;
        // bundle_id 服务端派生优先（R11 信任边界）：从校验过的 canonical 路径读真实 bundle ID，
        // 不信任前端回传——前端若传过宽前缀（如 "com"）会让 find_leftovers 前缀匹配到无关应用的
        // 残留、一并移废纸篓。派生不到（无 Info.plist）才回退前端值：那类应用本就无 bundle ID、
        // 解析不出残留（R7 优雅降级）。
        let resolved_bid = Engine::bundle_id_at(&canonical).or(bundle_id);
        // app bundle 本体作为 Safe 项在前（对齐 CLI uninstall.rs 的合成顺序）。
        let app_item = ScanItem::new(canonical, app_size, SafetyLevel::Safe, "应用".to_string());
        let mut categories = vec![CategoryGroup::new("应用".to_string(), vec![app_item])];
        // 残留作为单个 CategoryGroup 收纳——前端按各项自身的 category（如「应用残留 (Caches)」）
        // 重新分组渲染，故后端分组结构不影响展示、总量与按路径取项，无需在此按子目录再拆组。
        if let Some(bid) = resolved_bid.as_deref().filter(|s| !s.is_empty()) {
            let leftovers = Engine::find_leftovers(bid);
            if !leftovers.is_empty() {
                categories.push(CategoryGroup::new("应用残留".to_string(), leftovers));
            }
        }
        Ok::<ScanResult, String>(ScanResult::from_categories(categories))
    })
    .await
    .map_err(|e| format!("解析残留线程异常: {e}"))??;
    // 代次守卫写槽：乱序完成时旧扫描不覆盖新结果（见 slot.rs）。
    last_uninstall.commit(ticket, result.clone())?;
    Ok(result)
}

/// 阶段三：移废纸篓删除选中路径集（恒用 `DeleteMode::Trash`——GUI 无永久删除，R10）。
/// 待删项从 `last_uninstall` 按路径精确取出（R11，不接受前端回传完整 `ScanItem`）；
/// 含 Risky 时后端二次校验确认口令（复用 `authorize_deletion`），防前端绕过。
#[tauri::command]
pub async fn uninstall(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanReport, String> {
    let (cancelled, last_uninstall) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_uninstall.clone())
    };
    let selected: HashSet<PathBuf> = paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 短临界区 clone 出 owned 待删项后立即 drop 锁（与 clean/purge 同理，避免删除全程持锁）。
        let items: Vec<ScanItem> = {
            let guard = last_uninstall.read()?;
            let scan = guard.1.as_ref().ok_or_else(|| "无残留结果可清理".to_string())?;
            select_by_paths(scan, &selected).into_iter().cloned().collect()
        };
        authorize_deletion(&items, &confirm_token)?;
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("卸载失败: {e}"))
    })
    .await
    .map_err(|e| format!("卸载线程异常: {e}"))?
}

/// 校验前端传来的 app bundle 路径：须为存在的 `.app`，且 `canonicalize` 后落在
/// `/Applications` 或 `~/Applications` 下。防前端注入任意路径成为 Safe 预选删除项（R11）。
fn validate_app_path(app_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(app_path);
    if path.extension().is_none_or(|e| e != "app") {
        return Err("目标不是 .app 应用包".to_string());
    }
    let canonical = path
        .canonicalize()
        .map_err(|_| "应用路径不存在或不可解析".to_string())?;
    if !canonical.is_dir() {
        return Err("应用路径不是目录".to_string());
    }
    // 与应用发现口径一致：list_apps 用 platform::get_home_dir() 构造 ~/Applications
    // （dirs::home_dir 带 passwd 兜底）；用同一 helper 避免 HOME 未设时校验漏掉这些应用。
    let allowed = [
        PathBuf::from("/Applications"),
        mc_core::platform::get_home_dir().join("Applications"),
    ];
    let ok = allowed.iter().any(|base| {
        base.canonicalize()
            .is_ok_and(|b| canonical.starts_with(&b))
    });
    if !ok {
        return Err("应用路径不在 /Applications 或 ~/Applications 下".to_string());
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, safety: SafetyLevel) -> ScanItem {
        ScanItem::new(PathBuf::from(path), 100, safety, "应用".into())
    }

    fn scan_with(items: Vec<ScanItem>) -> ScanResult {
        ScanResult::from_categories(vec![CategoryGroup::new("应用".into(), items)])
    }

    /// AE6：uninstall 删除只从 uninstall 结果取项——clean 结果里的路径在 uninstall 槽中不命中。
    #[test]
    fn uninstall_selection_is_isolated_from_clean_result() {
        let uninstall_scan = scan_with(vec![item("/Applications/Foo.app", SafetyLevel::Safe)]);
        let clean_paths: HashSet<PathBuf> = ["/c/cache"].iter().map(PathBuf::from).collect();
        assert!(
            select_by_paths(&uninstall_scan, &clean_paths).is_empty(),
            "clean 结果的路径不得从 uninstall 槽命中（隔离）"
        );
    }

    /// R10：含 Risky 且口令无效 → 拒删；口令有效（trim/大小写不敏感）→ 放行。
    #[test]
    fn risky_items_require_valid_confirm_token() {
        let risky = vec![item("/Applications/Foo.app", SafetyLevel::Risky)];
        assert!(authorize_deletion(&risky, "").is_err());
        assert!(authorize_deletion(&risky, "delete").is_ok());
        assert!(authorize_deletion(&risky, "  DELETE\n").is_ok());
    }

    /// R11：非 .app 路径被拒绝，不进入删除槽。
    #[test]
    fn validate_rejects_non_app_path() {
        assert!(validate_app_path("/Applications/Foo.txt").is_err());
        assert!(validate_app_path("/does/not/exist.app").is_err());
    }

    /// R11：存在但不在 /Applications 或 ~/Applications 下的 .app 被拒绝（防注入任意路径）。
    #[test]
    fn validate_rejects_app_outside_applications_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let fake_app = dir.path().join("Evil.app");
        std::fs::create_dir(&fake_app).unwrap();
        assert!(
            validate_app_path(fake_app.to_str().unwrap()).is_err(),
            "临时目录下的 .app 不在 Applications 下，须被拒绝"
        );
    }

}
