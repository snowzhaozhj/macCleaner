use crate::cleaner::Cleaner;
use crate::models::{CleanReport, DeleteMode, ScanItem, ScanResult};
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
