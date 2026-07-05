use crate::models::{CleanReport, CleanedItem, DeleteMode, ScanItem};
use crate::progress::{ProgressEvent, ProgressReporter};
use std::path::Path;

/// 清理执行器，支持移到废纸篓和永久删除两种模式
pub struct Cleaner;

impl Cleaner {
    /// 对选中的项目执行清理操作
    pub fn execute(
        items: &[&ScanItem],
        mode: DeleteMode,
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<CleanReport> {
        let mut report = CleanReport::default();

        for item in items {
            reporter.on_event(ProgressEvent::CleaningFile {
                path: item.path.clone(),
            });

            let result = match mode {
                DeleteMode::Trash => Self::move_to_trash(&item.path),
                DeleteMode::Permanent => Self::permanent_delete(&item.path),
            };

            match result {
                Ok(()) => {
                    report.add(CleanedItem {
                        path: item.path.clone(),
                        size: item.size,
                        success: true,
                        error: None,
                    });
                }
                Err(e) => {
                    // 优雅降级：记录失败，继续处理剩余项目
                    log::warn!("清理失败 {:?}: {}", item.path, e);
                    report.add(CleanedItem {
                        path: item.path.clone(),
                        size: item.size,
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        let deleted_paths: Vec<std::path::PathBuf> = report
            .cleaned
            .iter()
            .filter(|c| c.success)
            .map(|c| c.path.clone())
            .collect();
        reporter.on_event(ProgressEvent::CleaningDone {
            freed: report.total_freed,
            count: report.success_count,
            deleted_paths,
        });

        Ok(report)
    }

    /// 试运行：构建报告但不实际删除文件
    pub fn dry_run(items: &[&ScanItem]) -> CleanReport {
        let mut report = CleanReport::default();
        for item in items {
            report.add(CleanedItem {
                path: item.path.clone(),
                size: item.size,
                success: true,
                error: None,
            });
        }
        report
    }

    fn move_to_trash(path: &Path) -> anyhow::Result<()> {
        trash::delete(path)?;
        Ok(())
    }

    fn permanent_delete(path: &Path) -> anyhow::Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SafetyLevel;
    use crate::progress::NoopReporter;
    use std::fs;
    use tempfile::tempdir;

    /// 辅助函数：创建测试用 `ScanItem`
    fn make_item(path: std::path::PathBuf, size: u64) -> ScanItem {
        ScanItem::new(path, size, SafetyLevel::Safe, "test".to_string())
    }

    #[test]
    fn test_dry_run_creates_report_without_deleting() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "hello").unwrap();

        let item = make_item(file_path.clone(), 5);
        let report = Cleaner::dry_run(&[&item]);

        // 报告应包含所有项目且标记为成功
        assert_eq!(report.cleaned.len(), 1);
        assert!(report.cleaned[0].success);
        assert_eq!(report.success_count, 1);
        assert_eq!(report.failure_count, 0);
        assert_eq!(report.total_freed, 5);

        // 文件不应被删除
        assert!(file_path.exists());
    }

    #[test]
    fn test_permanent_delete_removes_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("to_delete.txt");
        fs::write(&file_path, "bye").unwrap();
        assert!(file_path.exists());

        let item = make_item(file_path.clone(), 3);
        let reporter = NoopReporter;
        let report = Cleaner::execute(&[&item], DeleteMode::Permanent, &reporter).unwrap();

        assert_eq!(report.success_count, 1);
        assert_eq!(report.failure_count, 0);
        assert!(!file_path.exists());
    }

    #[test]
    fn test_permanent_delete_removes_directory() {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("subdir");
        fs::create_dir(&sub_dir).unwrap();
        fs::write(sub_dir.join("inner.txt"), "data").unwrap();
        assert!(sub_dir.exists());

        let item = make_item(sub_dir.clone(), 100);
        let reporter = NoopReporter;
        let report = Cleaner::execute(&[&item], DeleteMode::Permanent, &reporter).unwrap();

        assert_eq!(report.success_count, 1);
        assert!(!sub_dir.exists());
    }

    #[test]
    fn test_trash_mode_removes_from_original_path() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("trash_me.txt");
        fs::write(&file_path, "trash content").unwrap();
        assert!(file_path.exists());

        let item = make_item(file_path.clone(), 13);
        let reporter = NoopReporter;
        let report = Cleaner::execute(&[&item], DeleteMode::Trash, &reporter).unwrap();

        assert_eq!(report.success_count, 1);
        assert_eq!(report.failure_count, 0);
        // 原始路径应不再存在（已移到废纸篓）
        assert!(!file_path.exists());
    }

    #[test]
    fn test_permission_failure_produces_failed_item() {
        let non_existent = std::path::PathBuf::from("/tmp/mac_cleaner_non_existent_file_12345");
        // 确保路径确实不存在
        assert!(!non_existent.exists());

        let item = make_item(non_existent.clone(), 0);
        let reporter = NoopReporter;
        let report = Cleaner::execute(&[&item], DeleteMode::Permanent, &reporter).unwrap();

        assert_eq!(report.success_count, 0);
        assert_eq!(report.failure_count, 1);
        assert!(!report.cleaned[0].success);
        assert!(report.cleaned[0].error.is_some());
    }

    #[test]
    fn test_clean_zero_items_returns_empty_report() {
        let reporter = NoopReporter;
        let items: Vec<&ScanItem> = vec![];
        let report = Cleaner::execute(&items, DeleteMode::Permanent, &reporter).unwrap();

        assert_eq!(report.cleaned.len(), 0);
        assert_eq!(report.success_count, 0);
        assert_eq!(report.failure_count, 0);
        assert_eq!(report.total_freed, 0);
    }

    #[test]
    fn test_mixed_success_and_failure() {
        let dir = tempdir().unwrap();

        // 存在的文件 -> 应成功
        let good_path = dir.path().join("good.txt");
        fs::write(&good_path, "ok").unwrap();
        let good_item = make_item(good_path.clone(), 2);

        // 不存在的文件 -> 应失败
        let bad_path = dir.path().join("nonexistent.txt");
        let bad_item = make_item(bad_path, 99);

        // 再一个存在的文件 -> 应成功
        let good2_path = dir.path().join("good2.txt");
        fs::write(&good2_path, "ok2").unwrap();
        let good2_item = make_item(good2_path.clone(), 3);

        let reporter = NoopReporter;
        let report = Cleaner::execute(
            &[&good_item, &bad_item, &good2_item],
            DeleteMode::Permanent,
            &reporter,
        )
        .unwrap();

        assert_eq!(report.success_count, 2);
        assert_eq!(report.failure_count, 1);
        assert_eq!(report.total_freed, 2 + 3);
        assert_eq!(report.cleaned.len(), 3);

        // 验证顺序和状态
        assert!(report.cleaned[0].success);
        assert!(!report.cleaned[1].success);
        assert!(report.cleaned[2].success);
    }

    #[test]
    fn test_dry_run_structure_matches_execute() {
        let dir = tempdir().unwrap();
        let f1 = dir.path().join("a.txt");
        let f2 = dir.path().join("b.txt");
        fs::write(&f1, "aaa").unwrap();
        fs::write(&f2, "bbb").unwrap();

        let item1 = make_item(f1.clone(), 3);
        let item2 = make_item(f2.clone(), 3);

        let report = Cleaner::dry_run(&[&item1, &item2]);

        // dry_run 只构建报告，不改变结构
        assert_eq!(report.cleaned.len(), 2);
        assert_eq!(report.success_count, 2);
        assert_eq!(report.failure_count, 0);
        assert_eq!(report.total_freed, 6);

        // 文件仍在
        assert!(f1.exists());
        assert!(f2.exists());
    }
}
