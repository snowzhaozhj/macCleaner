use crate::models::{CleanReport, CleanedItem, DeleteMode, ScanItem};
use crate::progress::{ProgressEvent, ProgressReporter};
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

            // Trash 模式下捕获废纸篓落点（供 `mc undo` 确定性放回）；永久删除恒 None。
            let result = match mode {
                DeleteMode::Trash => Self::move_to_trash(&item.path),
                DeleteMode::Permanent => Self::permanent_delete(&item.path).map(|()| None),
            };

            match result {
                Ok(trashed_to) => {
                    report.add(CleanedItem {
                        path: item.path.clone(),
                        size: item.size,
                        success: true,
                        error: None,
                        trashed_to,
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
                        trashed_to: None,
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
                trashed_to: None,
            });
        }
        report
    }

    /// 移入废纸篓，并尽力捕获落点路径（`~/.Trash/<name>`）。
    ///
    /// 捕获策略：删除前后各读一次 `~/.Trash` 顶层名集合，取差集中 basename 词干匹配原文件名的
    /// **唯一**新条目。差集为空/多义/读目录失败 → 返回 `Ok(None)`（诚实降级，绝不猜）。
    /// 捕获失败不影响删除成功——删除本身若失败才返回 `Err`。
    fn move_to_trash(path: &Path) -> anyhow::Result<Option<PathBuf>> {
        let trash = crate::platform::trash_dir();
        let before = read_trash_names(&trash);
        trash::delete(path)?;
        // 删除成功后再快照，差集即本次新增。before 为 None（读不到废纸篓）时不尝试捕获。
        let dest = before.and_then(|before| {
            let after = read_trash_names(&trash)?;
            path.file_name()
                .and_then(|name| pick_new_trash_entry(&trash, &before, &after, name))
        });
        Ok(dest)
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

/// 读 `~/.Trash` 顶层条目名集合。读不到（目录不存在、无权限）→ `None`，调用方据此放弃捕获。
fn read_trash_names(trash: &Path) -> Option<HashSet<OsString>> {
    let entries = std::fs::read_dir(trash).ok()?;
    Some(
        entries
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .collect(),
    )
}

/// 从"删除前后 `~/.Trash` 名集合"里挑出本次删除产生的落点。
///
/// 落点判定：`after \ before`（新增项）中，名字与原文件名词干匹配的**唯一**一个才算数——
/// - 精确等于原名（无碰撞重命名）；或
/// - 以 `"{stem} "` 开头（macOS 碰撞重命名如 `foo.log` → `foo 2.log`、`node_modules` → `node_modules 2`）。
///
/// 匹配到 0 个或 >1 个都返回 `None`：并发进程（如 Finder）可能同一瞬间往废纸篓丢无关新条目，
/// 词干匹配把这类噪声排除；仍歧义时宁可不猜（`mc undo` 会对无落点项降级到 Finder 放回）。
fn pick_new_trash_entry(
    trash: &Path,
    before: &HashSet<OsString>,
    after: &HashSet<OsString>,
    original_name: &std::ffi::OsStr,
) -> Option<PathBuf> {
    let original = original_name.to_string_lossy();
    // 词干 = 原名去掉最后一段扩展名（无扩展名则为整名），用于匹配碰撞重命名的 "stem N.ext"。
    let stem = Path::new(original_name)
        .file_stem()
        .map_or_else(|| original.to_string(), |s| s.to_string_lossy().into_owned());
    let collision_prefix = format!("{stem} ");

    let mut matches = after
        .difference(before)
        .filter(|name| {
            let n = name.to_string_lossy();
            *n == *original || n.starts_with(&collision_prefix)
        });
    let first = matches.next()?;
    // 存在第二个匹配 → 歧义，放弃。
    if matches.next().is_some() {
        return None;
    }
    Some(trash.join(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SafetyLevel;
    use crate::progress::NoopReporter;
    use std::fs;
    use tempfile::tempdir;

    fn names(list: &[&str]) -> HashSet<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn pick_trash_entry_exact_match_single_new() {
        // 无碰撞：落点名精确等于原文件名。
        let before = names(&["old.txt"]);
        let after = names(&["old.txt", "report.log"]);
        let dest = pick_new_trash_entry(
            Path::new("/T"),
            &before,
            &after,
            std::ffi::OsStr::new("report.log"),
        );
        assert_eq!(dest, Some(PathBuf::from("/T/report.log")));
    }

    #[test]
    fn pick_trash_entry_collision_rename_with_space_number() {
        // 碰撞：macOS 把 report.log 重命名为 "report 2.log"，词干 "report" + 空格前缀命中。
        let before = names(&["report.log"]);
        let after = names(&["report.log", "report 2.log"]);
        let dest = pick_new_trash_entry(
            Path::new("/T"),
            &before,
            &after,
            std::ffi::OsStr::new("report.log"),
        );
        assert_eq!(dest, Some(PathBuf::from("/T/report 2.log")));
    }

    #[test]
    fn pick_trash_entry_directory_collision() {
        // 目录（无扩展名）碰撞："node_modules" → "node_modules 2"。
        let before = names(&[]);
        let after = names(&["node_modules 2"]);
        let dest = pick_new_trash_entry(
            Path::new("/T"),
            &before,
            &after,
            std::ffi::OsStr::new("node_modules"),
        );
        assert_eq!(dest, Some(PathBuf::from("/T/node_modules 2")));
    }

    #[test]
    fn pick_trash_entry_empty_diff_is_none() {
        // 删除未产生新条目（如落到别的卷）→ 不可确定。
        let before = names(&["a", "b"]);
        let after = names(&["a", "b"]);
        assert_eq!(
            pick_new_trash_entry(Path::new("/T"), &before, &after, std::ffi::OsStr::new("a")),
            None
        );
    }

    #[test]
    fn pick_trash_entry_ignores_unrelated_concurrent_noise() {
        // 差集里有并发进程丢进来的无关新条目，但仅一项词干匹配原名 → 仍能确定落点。
        let before = names(&["x"]);
        let after = names(&["x", "cache.db", "unrelated-from-finder"]);
        let dest = pick_new_trash_entry(
            Path::new("/T"),
            &before,
            &after,
            std::ffi::OsStr::new("cache.db"),
        );
        assert_eq!(dest, Some(PathBuf::from("/T/cache.db")));
    }

    #[test]
    fn pick_trash_entry_ambiguous_multiple_matches_is_none() {
        // 两个新条目都词干匹配（精确 + 碰撞重命名）→ 歧义，宁可不猜。
        let before = names(&[]);
        let after = names(&["data.log", "data 2.log"]);
        assert_eq!(
            pick_new_trash_entry(
                Path::new("/T"),
                &before,
                &after,
                std::ffi::OsStr::new("data.log")
            ),
            None
        );
    }

    #[test]
    fn read_trash_names_missing_dir_is_none() {
        // 读不到废纸篓目录（不存在）→ None，调用方据此放弃捕获。
        assert!(read_trash_names(Path::new("/nonexistent/.Trash/xyz123")).is_none());
    }

    #[test]
    fn read_trash_names_lists_entries() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        let got = read_trash_names(dir.path()).unwrap();
        assert!(got.contains(&OsString::from("a.txt")));
        assert!(got.contains(&OsString::from("sub")));
    }

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
        // 用唯一文件名，避免与用户废纸篓既有项碰撞导致落点歧义。
        let unique = format!("mc_trash_capture_{}.txt", std::process::id());
        let file_path = dir.path().join(&unique);
        fs::write(&file_path, "trash content").unwrap();
        assert!(file_path.exists());

        let item = make_item(file_path.clone(), 13);
        let reporter = NoopReporter;
        let report = Cleaner::execute(&[&item], DeleteMode::Trash, &reporter).unwrap();

        assert_eq!(report.success_count, 1);
        assert_eq!(report.failure_count, 0);
        // 原始路径应不再存在（已移到废纸篓）
        assert!(!file_path.exists());

        // 端到端：应捕获到废纸篓落点，且落点确实存在于 ~/.Trash 下。
        let dest = report.cleaned[0]
            .trashed_to
            .clone()
            .expect("Trash 删除应捕获到落点");
        assert!(dest.starts_with(crate::platform::trash_dir()), "落点应在 ~/.Trash 下: {dest:?}");
        assert!(dest.exists(), "捕获到的落点应真实存在: {dest:?}");
        // 清理：把测试项从用户废纸篓移走，避免污染。
        let _ = fs::remove_file(&dest);
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
    fn execute_cleaning_done_deleted_paths_contains_only_successful() {
        use crate::progress::ProgressReporter;
        use std::sync::Mutex;

        // 捕获 CleaningDone 事件，断言 deleted_paths 的 filter(success) 生产逻辑——
        // 它是 TUI 端分析器剪树安全性的唯一数据源：失败项绝不能出现在里面，
        // 否则会误剪掉磁盘上仍存在的目录。
        #[derive(Default)]
        struct CapturingReporter {
            done: Mutex<Option<(u64, usize, Vec<std::path::PathBuf>)>>,
        }
        impl ProgressReporter for CapturingReporter {
            fn on_event(&self, event: ProgressEvent) {
                if let ProgressEvent::CleaningDone {
                    freed,
                    count,
                    deleted_paths,
                } = event
                {
                    *self.done.lock().unwrap() = Some((freed, count, deleted_paths));
                }
            }
        }

        let dir = tempdir().unwrap();
        let good = dir.path().join("good.txt");
        fs::write(&good, "ok").unwrap();
        let good_item = make_item(good.clone(), 2);
        // 不存在的路径 → 删除失败，绝不能进入 deleted_paths
        let bad = dir.path().join("nonexistent.txt");
        let bad_item = make_item(bad.clone(), 99);
        let good2 = dir.path().join("good2.txt");
        fs::write(&good2, "ok2").unwrap();
        let good2_item = make_item(good2.clone(), 3);

        let reporter = CapturingReporter::default();
        Cleaner::execute(
            &[&good_item, &bad_item, &good2_item],
            DeleteMode::Permanent,
            &reporter,
        )
        .unwrap();

        let (freed, count, deleted_paths) = reporter
            .done
            .lock()
            .unwrap()
            .clone()
            .expect("execute 应发出 CleaningDone");
        assert_eq!(count, 2);
        assert_eq!(freed, 2 + 3);
        assert_eq!(deleted_paths.len(), 2, "只应含两个成功项");
        assert!(deleted_paths.contains(&good));
        assert!(deleted_paths.contains(&good2));
        assert!(
            !deleted_paths.contains(&bad),
            "失败项不得出现在 deleted_paths（否则 TUI 会误剪存活目录）"
        );
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
