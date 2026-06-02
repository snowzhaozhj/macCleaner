use crate::models::ScanResult;
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

    // analyze 和 clean 执行方法将在 U5/U8 中添加
}
