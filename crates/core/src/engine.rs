use crate::app_resolver::AppResolver;
use crate::cleaner::Cleaner;
use crate::models::{AppInfo, CleanReport, DeleteMode, ScanItem, ScanResult};
use crate::progress::ProgressReporter;
use crate::scanner::Scanner;
use std::path::Path;

/// 主引擎，作为 UI 层的统一入口
pub struct Engine;

impl Engine {
    /// 执行 clean 扫描（系统缓存、浏览器缓存等）
    pub fn scan_clean(reporter: &dyn ProgressReporter) -> anyhow::Result<ScanResult> {
        Scanner::scan_clean(reporter)
    }

    /// 执行 purge 扫描（开发产物目录）
    pub fn scan_purge(path: &Path, reporter: &dyn ProgressReporter) -> anyhow::Result<ScanResult> {
        Scanner::scan_purge(path, reporter)
    }

    /// 流式扫描已安装应用（用于 TUI Uninstall 后台线程，边扫边报）
    pub fn scan_uninstall(reporter: &dyn ProgressReporter) {
        AppResolver::scan_apps_streaming(reporter);
    }

    /// 列出已安装应用（同步，含 `bundle_id`）。facade 平价：委托 `AppResolver::list_apps`，无逻辑。
    ///
    /// GUI Uninstall 走此同步路径而非 `scan_uninstall` 流式：后者的 `Found` 事件丢弃
    /// `bundle_id`，而残留解析（`find_leftovers`）必须要它。CLI `mc uninstall` 同理用它。
    #[must_use]
    pub fn list_apps() -> Vec<AppInfo> {
        AppResolver::list_apps()
    }

    /// 按 `bundle_id` 解析应用残留。facade 平价：委托 `AppResolver::find_leftovers`，无逻辑。
    ///
    /// 残留项的 `SafetyLevel`/预选/证据由 `AppResolver` 决定（用户数据残留 Moderate 不预选）。
    #[must_use]
    pub fn find_leftovers(bundle_id: &str) -> Vec<ScanItem> {
        AppResolver::find_leftovers(bundle_id)
    }

    /// 同 `find_leftovers`，但附带因 `PermissionDenied` 跳过的路径（#23）。GUI 用此变体展示「因权限跳过」区。
    #[must_use]
    pub fn find_leftovers_with_skips(bundle_id: &str) -> (Vec<ScanItem>, Vec<std::path::PathBuf>) {
        AppResolver::find_leftovers_with_skips(bundle_id)
    }

    /// 反向卸载：扫描父 App 已不存在的孤儿残留。facade 平价：委托 `AppResolver::scan_orphans`，无逻辑。
    ///
    /// 孤儿项一律不预选（`AppResolver` 决定），删除授权侧不受影响（仍只信内置规则）。
    #[must_use]
    pub fn scan_orphans() -> Vec<ScanItem> {
        AppResolver::scan_orphans()
    }

    /// 同 `scan_orphans`，但附带因 `PermissionDenied` 跳过的路径（#23）。GUI 用此变体展示「因权限跳过」区。
    #[must_use]
    pub fn scan_orphans_with_skips() -> (Vec<ScanItem>, Vec<std::path::PathBuf>) {
        AppResolver::scan_orphans_with_skips()
    }

    /// 读取 `.app` 的真实 bundle ID（只解析 Info.plist）。facade 平价：委托 `AppResolver`。
    ///
    /// GUI 卸载用它服务端派生 bundle ID，不信任前端回传的过宽前缀（防误匹配他应用残留）。
    #[must_use]
    pub fn bundle_id_at(app_path: &Path) -> Option<String> {
        AppResolver::bundle_id_at(app_path)
    }

    /// 执行清理操作（实际删除文件）
    pub fn clean(
        items: &[&ScanItem],
        mode: DeleteMode,
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<CleanReport> {
        Cleaner::execute(items, mode, reporter)
    }

    /// 试运行：返回清理报告但不删除任何文件
    pub fn dry_run(items: &[&ScanItem]) -> CleanReport {
        Cleaner::dry_run(items)
    }
}

#[cfg(test)]
mod tests {
    use super::Engine;

    /// facade 平价：`Engine::list_apps` 与 `AppResolver::list_apps` 在同环境下等价（委托生效）。
    #[test]
    fn list_apps_delegates_to_app_resolver() {
        let via_engine = Engine::list_apps();
        let via_resolver = crate::app_resolver::AppResolver::list_apps();
        assert_eq!(
            via_engine.len(),
            via_resolver.len(),
            "Engine::list_apps 应原样委托 AppResolver::list_apps"
        );
    }

    /// facade 平价：`Engine::find_leftovers` 对不存在 bundle 返回空，委托生效且不 panic。
    #[test]
    fn find_leftovers_delegates_and_handles_unknown_bundle() {
        let leftovers = Engine::find_leftovers("com.test.nonexistent.app.mc12345");
        assert!(
            leftovers.is_empty(),
            "未知 bundle_id 应无残留（委托 AppResolver::find_leftovers）"
        );
    }

    /// facade 平价：`Engine::bundle_id_at` 对不存在的 .app 返回 None，委托生效且不 panic。
    #[test]
    fn bundle_id_at_delegates_and_handles_missing_app() {
        let bid = Engine::bundle_id_at(std::path::Path::new("/does/not/exist.app"));
        assert!(bid.is_none(), "不存在的 .app 应返回 None（委托 AppResolver::bundle_id_at）");
    }

    /// facade 平价：`Engine::scan_orphans` 与 `AppResolver::scan_orphans` 在同环境下等价，
    /// 委托生效、不 panic（真机 `~/Library` 有无孤儿都稳健）。
    #[test]
    fn scan_orphans_delegates_to_app_resolver() {
        let via_engine = Engine::scan_orphans();
        let via_resolver = crate::app_resolver::AppResolver::scan_orphans();
        assert_eq!(
            via_engine.len(),
            via_resolver.len(),
            "Engine::scan_orphans 应原样委托 AppResolver::scan_orphans"
        );
    }
}
