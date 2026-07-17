//! 从废纸篓确定性放回上次清理的项（`mc undo` 的核心引擎）。
//!
//! 输入是账本里的 `RestoreEntry`（原始路径 → 废纸篓落点 + inode，见 `history` 与 `cleaner` 的落点捕获）。
//! 逐项校验后 `fs::rename` 放回，镜像 `cleaner` 的**优雅降级**契约：单项失败只记录、继续处理其余项，
//! 绝不 panic、绝不中断整批。
//!
//! 安全不变量：
//! - **身份校验防名字复用**：macOS 清空废纸篓后会**复用名字**，仅凭 `trashed_to` 路径可能指向一个无关的同名
//!   新文件。恢复前比对落点当前 inode 与账本记录的 `trashed_ino`，不符即视为"当初那个文件已不在"跳过——
//!   否则会把无关文件误恢复到原址（审查 headline）。
//! - **绝不覆盖**：原址当前存在任何条目（含损坏符号链接）即跳过，把选择权交回用户（避免二次数据丢失）。
//! - **只放回仍在废纸篓的项**：落点已不存在（用户清空废纸篓/手动放回）即跳过。
//! - **跨卷不做复制回退**：`rename` 失败（如落点与原址不同卷）记为 `Failed`，不引入半复制的新风险面。
//!
//! 信任边界：`trashed_to`/`original` 均来自用户自己机器上的账本文件（`~/.local/state/mc/history.jsonl`），
//! undo 以用户自身权限运行、不跨权限边界，故不对账本内容做路径合法性校验（篡改自己的账本移动自己的文件
//! 不构成提权）。勿在此处半吊子地"加固"路径校验而误以为已验证。

use std::os::unix::fs::MetadataExt;
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
    let Ok(meta) = entry.trashed_to.symlink_metadata() else {
        return make(RestoreStatus::SkippedTrashMissing, None);
    };
    // 身份校验：落点当前 inode 必须与账本记录一致。macOS 清空废纸篓后复用名字，
    // 不符说明这个名字下已是另一个无关文件——当初那个已不在，按"废纸篓中已无此项"跳过，绝不误恢复。
    if meta.ino() != entry.trashed_ino {
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
    /// `trashed_ino` 取落点文件的真实 inode，使身份校验通过（模拟"账本记录的就是这个文件"）。
    fn entry(trash_dir: &std::path::Path, orig_dir: &std::path::Path, name: &str) -> RestoreEntry {
        let trashed_to = trash_dir.join(name);
        fs::write(&trashed_to, "content").unwrap();
        let trashed_ino = fs::symlink_metadata(&trashed_to).unwrap().ino();
        RestoreEntry {
            original: orig_dir.join(name),
            trashed_to,
            trashed_ino,
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
        let (trashed_to, trashed_ino) = {
            let p = trash.path().join("a.txt");
            fs::write(&p, "x").unwrap();
            let ino = fs::symlink_metadata(&p).unwrap().ino();
            (p, ino)
        };
        let e = RestoreEntry {
            original: deep_parent.join("a.txt"),
            trashed_to,
            trashed_ino,
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
            trashed_ino: 0, // 落点不存在，ino 无关（存在性检查先跳过）
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
            trashed_ino: 0,
        };

        let report = restore(&[ok.clone(), occupied.clone(), missing], false);

        assert_eq!(report.restored_count(), 1);
        assert_eq!(report.skipped_count(), 2);
        assert_eq!(report.failed_count(), 0);
        assert!(ok.original.exists());
        assert_eq!(fs::read_to_string(&occupied.original).unwrap(), "USER");
    }

    #[test]
    fn restore_skips_when_trash_ino_mismatches() {
        // headline 安全场景：macOS 清空废纸篓后名字被复用，落点路径下已是另一个无关同名文件。
        // 账本记录的 inode 与当前 inode 不符 → 必须跳过，绝不把无关文件恢复到原址。
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let mut e = entry(trash.path(), orig.path(), "cache.db");
        // 模拟身份不符：账本记录的 inode 与废纸篓当前文件的 inode 不同。
        e.trashed_ino = e.trashed_ino.wrapping_add(1);

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.restored_count(), 0);
        assert_eq!(report.outcomes[0].status, RestoreStatus::SkippedTrashMissing);
        assert!(!e.original.exists(), "身份不符时绝不恢复到原址");
        assert!(e.trashed_to.exists(), "废纸篓里的无关文件应原封不动");
    }

    #[test]
    fn restore_reports_failed_when_parent_creation_fails() {
        // 覆盖 Failed 路径：让原址父路径是一个已存在的普通文件，create_dir_all 必失败。
        let trash = tempdir().unwrap();
        let orig = tempdir().unwrap();
        let mut e = entry(trash.path(), orig.path(), "x.txt");
        let blocker = orig.path().join("blocker");
        fs::write(&blocker, "i am a file").unwrap();
        e.original = blocker.join("child/x.txt"); // 父路径 blocker 是文件 → 建目录失败

        let report = restore(std::slice::from_ref(&e), false);

        assert_eq!(report.failed_count(), 1);
        assert_eq!(report.outcomes[0].status, RestoreStatus::Failed);
        assert!(report.outcomes[0].error.is_some(), "失败应带错误详情");
        assert!(e.trashed_to.exists(), "失败时落点应留在废纸篓，数据不丢");
    }

    /// 端到端真实回路：`Cleaner`（真移废纸篓）→ `HistoryEntry::from_report`（建映射）→
    /// `restore`（放回）。用真实 `~/.Trash`，但恢复会把文件移回原址，故无废纸篓污染。
    #[test]
    fn end_to_end_clean_capture_record_restore_roundtrip() {
        use crate::cleaner::Cleaner;
        use crate::history::{HistoryCommand, HistoryEntry};
        use crate::models::{DeleteMode, SafetyLevel, ScanItem};
        use crate::progress::NoopReporter;

        let dir = tempdir().unwrap();
        let unique = format!("mc_undo_e2e_{}.txt", std::process::id());
        let file = dir.path().join(&unique);
        fs::write(&file, "precious").unwrap();

        // 1) 清理：移入废纸篓，捕获落点。
        let item = ScanItem::new(file.clone(), 8, SafetyLevel::Safe, "test".into());
        let report = Cleaner::execute(&[&item], DeleteMode::Trash, &NoopReporter).unwrap();
        assert!(!file.exists(), "清理后原址应消失");

        // 2) 记账：从报告构建 restorable 映射。
        let entry = HistoryEntry::from_report(HistoryCommand::Clean, &[&item], &report);
        assert_eq!(entry.restorable.len(), 1, "应记录一条可恢复映射");

        // 3) 撤销：放回原址。
        let restore_report = restore(&entry.restorable, false);
        assert_eq!(restore_report.restored_count(), 1);
        assert!(file.exists(), "撤销后文件应回到原址");
        assert_eq!(fs::read_to_string(&file).unwrap(), "precious", "内容应完好");
    }
}
