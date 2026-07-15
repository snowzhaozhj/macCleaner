//! 从废纸篓确定性放回上次清理的项（`mc undo` 的核心引擎）。
//!
//! 输入是账本里的 `RestoreEntry`（原始路径 → 废纸篓落点，见 `history` 与 `cleaner` 的落点捕获）。
//! 逐项校验后 `fs::rename` 放回，镜像 `cleaner` 的**优雅降级**契约：单项失败只记录、继续处理其余项，
//! 绝不 panic、绝不中断整批。
//!
//! 安全不变量：
//! - **绝不覆盖**：原址当前存在任何条目（含损坏符号链接）即跳过，把选择权交回用户（避免二次数据丢失）。
//! - **只放回仍在废纸篓的项**：落点已不存在（用户清空废纸篓/手动放回）即跳过。
//! - **跨卷不做复制回退**：`rename` 失败（如落点与原址不同卷）记为 `Failed`，不引入半复制的新风险面。

use std::path::PathBuf;

use serde::Serialize;

use crate::history::RestoreEntry;

/// 单项恢复结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RestoreStatus {
    /// 已放回原址（`dry_run` 下表示"将放回"）。
    Restored,
    /// 原址已被占用，跳过——绝不覆盖。
    SkippedTargetOccupied,
    /// 废纸篓落点已不存在（已清空/已手动放回），跳过。
    SkippedTrashMissing,
    /// 放回动作本身失败（如跨卷 rename）。详情见 `error`。
    Failed,
}

/// 一项恢复的完整结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreOutcome {
    pub original: PathBuf,
    pub trashed_to: PathBuf,
    pub status: RestoreStatus,
    pub error: Option<String>,
}

/// 一次 `restore` 的汇总报告。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RestoreReport {
    pub outcomes: Vec<RestoreOutcome>,
    /// 本次是否为预览（不实际移动文件）。
    pub dry_run: bool,
}

impl RestoreReport {
    #[must_use]
    pub fn restored_count(&self) -> usize {
        self.count(RestoreStatus::Restored)
    }

    #[must_use]
    pub fn skipped_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| {
                matches!(
                    o.status,
                    RestoreStatus::SkippedTargetOccupied | RestoreStatus::SkippedTrashMissing
                )
            })
            .count()
    }

    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.count(RestoreStatus::Failed)
    }

    fn count(&self, status: RestoreStatus) -> usize {
        self.outcomes.iter().filter(|o| o.status == status).count()
    }
}

/// 把一组 `RestoreEntry` 从废纸篓放回原址。
///
/// `dry_run == true` 时只做只读校验（落点在否、原址空否）并标注每项将执行的动作，不移动任何文件。
/// 返回逐项结果，永不返回 `Err`（单项失败降级为 `RestoreStatus::Failed`）。
#[must_use]
pub fn restore(entries: &[RestoreEntry], dry_run: bool) -> RestoreReport {
    let outcomes = entries
        .iter()
        .map(|e| restore_one(e, dry_run))
        .collect();
    RestoreReport { outcomes, dry_run }
}

fn restore_one(entry: &RestoreEntry, dry_run: bool) -> RestoreOutcome {
    let make = |status, error| RestoreOutcome {
        original: entry.original.clone(),
        trashed_to: entry.trashed_to.clone(),
        status,
        error,
    };

    // 用 symlink_metadata 判定存在性：能捕获损坏符号链接（`exists()` 会把它当不存在）。
    if entry.trashed_to.symlink_metadata().is_err() {
        return make(RestoreStatus::SkippedTrashMissing, None);
    }
    if entry.original.symlink_metadata().is_ok() {
        // 原址已被占（删除后用户又新建了同名项）：绝不覆盖。
        return make(RestoreStatus::SkippedTargetOccupied, None);
    }
    if dry_run {
        return make(RestoreStatus::Restored, None);
    }

    // 原址父目录可能已随删除消失，重建之。
    if let Some(parent) = entry.original.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return make(RestoreStatus::Failed, Some(format!("重建父目录失败: {e}")));
        }
    }
    match std::fs::rename(&entry.trashed_to, &entry.original) {
        Ok(()) => make(RestoreStatus::Restored, None),
        Err(e) => make(RestoreStatus::Failed, Some(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// 构造一个"废纸篓项 + 原址"的映射：在 `trash` 下建落点文件，`original` 指向 `orig`（默认不存在）。
    fn entry(trash_dir: &std::path::Path, orig_dir: &std::path::Path, name: &str) -> RestoreEntry {
        let trashed_to = trash_dir.join(name);
        fs::write(&trashed_to, "content").unwrap();
        RestoreEntry {
            original: orig_dir.join(name),
            trashed_to,
        }
    }

    #[test]
    fn restore_moves_trashed_file_back_to_original() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let e = entry(trash.path(), orig.path(), "a.txt");

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.restored_count(), 1);
        assert!(e.original.exists(), "原址应出现");
        assert!(!e.trashed_to.exists(), "落点应已移走");
        assert_eq!(fs::read_to_string(&e.original).unwrap(), "content");
    }

    #[test]
    fn restore_recreates_missing_parent_dir() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        // 原址父目录不存在。
        let deep_parent = orig.path().join("gone/deeper");
        let e = RestoreEntry {
            original: deep_parent.join("a.txt"),
            trashed_to: {
                let p = trash.path().join("a.txt");
                fs::write(&p, "x").unwrap();
                p
            },
        };
        assert!(!deep_parent.exists());

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.restored_count(), 1);
        assert!(e.original.exists(), "应重建父目录并放回");
    }

    #[test]
    fn restore_never_overwrites_occupied_target() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let e = entry(trash.path(), orig.path(), "a.txt");
        // 原址已被占：用户删除后又新建了同名文件，内容不同。
        fs::write(&e.original, "USER DATA").unwrap();

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.skipped_count(), 1);
        assert_eq!(report.outcomes[0].status, RestoreStatus::SkippedTargetOccupied);
        // 关键安全断言：原址内容绝不被覆盖，落点保持不动。
        assert_eq!(fs::read_to_string(&e.original).unwrap(), "USER DATA");
        assert!(e.trashed_to.exists(), "跳过时落点应原封不动");
    }

    #[test]
    fn restore_skips_when_trash_dest_missing() {
        let orig = tempdir().unwrap();
        // 落点从未创建（模拟用户已清空废纸篓）。
        let e = RestoreEntry {
            original: orig.path().join("a.txt"),
            trashed_to: orig.path().join("nonexistent-in-trash"),
        };

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.skipped_count(), 1);
        assert_eq!(report.outcomes[0].status, RestoreStatus::SkippedTrashMissing);
        assert!(!e.original.exists());
    }

    #[test]
    fn restore_dry_run_moves_nothing() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let e = entry(trash.path(), orig.path(), "a.txt");

        let report = restore(std::slice::from_ref(&e), true);

        assert!(report.dry_run);
        assert_eq!(report.restored_count(), 1, "预览应把可恢复项标为 Restored");
        // 但文件原封不动。
        assert!(e.trashed_to.exists(), "dry-run 不应移动落点");
        assert!(!e.original.exists(), "dry-run 不应创建原址");
    }

    #[test]
    fn restore_is_idempotent_second_run_all_trash_missing() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let e = entry(trash.path(), orig.path(), "a.txt");

        let first = restore(std::slice::from_ref(&e), false);
        assert_eq!(first.restored_count(), 1);

        // 第二次：落点已被第一次移走 → 全部跳过（TrashMissing），不误动已恢复的原址。
        let second = restore(std::slice::from_ref(&e), false);
        assert_eq!(second.restored_count(), 0);
        assert_eq!(second.outcomes[0].status, RestoreStatus::SkippedTrashMissing);
        assert!(e.original.exists(), "已恢复的原址不受二次 undo 影响");
    }

    #[test]
    fn restore_mixed_batch_counts_each_independently() {
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();

        // 1) 正常可恢复
        let ok = entry(trash.path(), orig.path(), "ok.txt");
        // 2) 原址被占
        let occupied = entry(trash.path(), orig.path(), "occupied.txt");
        fs::write(&occupied.original, "USER").unwrap();
        // 3) 落点缺失
        let missing = RestoreEntry {
            original: orig.path().join("missing.txt"),
            trashed_to: trash.path().join("never-existed"),
        };

        let report = restore(&[ok.clone(), occupied.clone(), missing], false);

        assert_eq!(report.restored_count(), 1);
        assert_eq!(report.skipped_count(), 2);
        assert_eq!(report.failed_count(), 0);
        assert!(ok.original.exists());
        assert_eq!(fs::read_to_string(&occupied.original).unwrap(), "USER");
    }
}
