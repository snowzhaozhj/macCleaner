//! Undo 命令：把某次 clean/purge 移入废纸篓的项确定性放回原处（GUI 一键撤销的后端）。
//!
//! 数据源是清理账本里的 `restorable` 映射（原始路径 → 废纸篓落点，见 `mc_core::history`）；
//! 恢复动作与安全护栏全在 `mc_core::restore`，本文件只负责"按回执 `run_id` 选条目 + 无落点降级"。
//!
//! **按 `run_id` 精确命中，而非全局最近**：账本是 CLI 与 GUI 共享的单一文件（`~/.local/state/mc/
//! history.jsonl`）。若取"全局最近含落点条目"，用户在 GUI 清理后、点撤销前于终端跑一次 `mc clean`
//! 就会让撤销劫持到那条 CLI 记录、放回无关文件。故 undo 收回执自身的 `run_id`，只撤销"这张回执那次"。
//!
//! 降级：`run_id` 未命中 / 命中条目无 `restorable`（落点未捕获）→ 返回空 `RestoreReport`，
//! 前端据此退回"在访达中恢复"手动路径（不假装成功、不放回别的清理）。

use mc_core::history;
use mc_core::restore::{self, RestoreReport};
use std::path::Path;

/// 纯核心（脱离 tauri，便于单测）：从 `ledger_path` 账本按 `run_id` 精确命中并放回。
///
/// 命中且有落点 → 逐项 restore；未命中/无落点 → 空报告（调用方据此走 Finder 降级）。
/// `restore::restore` 永不返回 Err（单项失败降级为 `Failed`），故本函数也不返回 Err。
fn undo_from_ledger(ledger_path: &Path, run_id: &str) -> RestoreReport {
    let entries = history::load(ledger_path);
    match history::select_entry(&entries, Some(run_id)) {
        Some(entry) if !entry.restorable.is_empty() => restore::restore(&entry.restorable, false),
        _ => RestoreReport::default(),
    }
}

/// 撤销 `run_id` 对应的那次清理：从废纸篓把该次移入的项放回原处。
///
/// 恒实际执行（非 dry-run）：GUI 撤销就是真放回。`fs::rename` 是阻塞 IO，
/// 放进 `spawn_blocking` 避免占用 async 运行时线程（与 clean/purge 一致）。
#[tauri::command]
pub async fn undo(run_id: String) -> Result<RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        undo_from_ledger(&history::default_path(), &run_id)
    })
    .await
    .map_err(|e| format!("撤销线程异常: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::history::{HistoryCommand, HistoryEntry, RestoreEntry};
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use tempfile::tempdir;

    /// 造一条含单个可恢复项的账本条目；落点是 `trash_dir/<name>`，inode 取真实文件的。
    fn entry_with_trashed(run_id: &str, original: &Path, trashed_to: &Path) -> HistoryEntry {
        let ino = fs::metadata(trashed_to).unwrap().ino();
        HistoryEntry {
            run_id: run_id.into(),
            timestamp: 1,
            command: HistoryCommand::Clean,
            freed: 0,
            count: 1,
            categories: vec![],
            deleted_paths: vec![original.to_path_buf()],
            restorable: vec![RestoreEntry {
                original: original.to_path_buf(),
                trashed_to: trashed_to.to_path_buf(),
                trashed_ino: ino,
            }],
        }
    }

    fn empty_entry(run_id: &str) -> HistoryEntry {
        HistoryEntry {
            run_id: run_id.into(),
            timestamp: 1,
            command: HistoryCommand::Clean,
            freed: 0,
            count: 0,
            categories: vec![],
            deleted_paths: vec![],
            restorable: vec![],
        }
    }

    fn write_ledger(path: &Path, entries: &[HistoryEntry]) {
        for e in entries {
            history::record(e, path).unwrap();
        }
    }

    #[test]
    fn undo_restores_matched_run_to_original_path() {
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl");
        let original = dir.path().join("orig.txt");
        let trashed = dir.path().join("trash/orig.txt");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::write(&trashed, b"payload").unwrap(); // 模拟已在废纸篓的文件

        write_ledger(&ledger, &[entry_with_trashed("r1", &original, &trashed)]);

        let report = undo_from_ledger(&ledger, "r1");
        assert_eq!(report.restored_count(), 1, "应放回 1 项");
        assert!(original.exists(), "文件应回到原路径");
        assert!(!trashed.exists(), "废纸篓落点应已移走");
    }

    #[test]
    fn undo_missing_run_id_returns_empty_report() {
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl");
        let original = dir.path().join("orig.txt");
        let trashed = dir.path().join("trash/orig.txt");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::write(&trashed, b"payload").unwrap();
        write_ledger(&ledger, &[entry_with_trashed("r1", &original, &trashed)]);

        // run_id 不存在 → 空报告，且不动任何文件（R4 降级触发点）。
        let report = undo_from_ledger(&ledger, "nope");
        assert_eq!(report.restored_count(), 0);
        assert!(report.outcomes.is_empty());
        assert!(!original.exists(), "未命中不应放回任何文件");
        assert!(trashed.exists(), "废纸篓落点应原样保留");
    }

    #[test]
    fn undo_matched_entry_without_restorable_returns_empty() {
        // 命中条目但 restorable 为空（落点未捕获）→ 空报告（R4 降级——旧全局选取会漏掉此分支）。
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl");
        write_ledger(&ledger, &[empty_entry("r1")]);

        let report = undo_from_ledger(&ledger, "r1");
        assert_eq!(report.restored_count(), 0);
        assert!(report.outcomes.is_empty(), "无落点应给空报告而非报错");
    }

    #[test]
    fn undo_ignores_newer_unrelated_entry() {
        // 共享账本竞态：给定旧 run_id，即便存在更新的、不同 run_id 的含落点条目，也只放回旧条目。
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl");

        let old_orig = dir.path().join("old.txt");
        let old_trash = dir.path().join("trash/old.txt");
        let new_orig = dir.path().join("new.txt");
        let new_trash = dir.path().join("trash/new.txt");
        fs::create_dir_all(old_trash.parent().unwrap()).unwrap();
        fs::write(&old_trash, b"old").unwrap();
        fs::write(&new_trash, b"new").unwrap();

        write_ledger(
            &ledger,
            &[
                entry_with_trashed("old", &old_orig, &old_trash),
                entry_with_trashed("newer", &new_orig, &new_trash),
            ],
        );

        let report = undo_from_ledger(&ledger, "old");
        assert_eq!(report.restored_count(), 1);
        assert!(old_orig.exists(), "只放回旧条目的项");
        assert!(!new_orig.exists(), "不得放回更新的无关条目");
        assert!(new_trash.exists(), "更新条目的落点应原样保留");
    }

    #[test]
    fn undo_skips_when_target_occupied() {
        // 原址已被占用 → restore 引擎跳过（不覆盖），报告如实透传 skipped。
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl");
        let original = dir.path().join("orig.txt");
        let trashed = dir.path().join("trash/orig.txt");
        fs::create_dir_all(trashed.parent().unwrap()).unwrap();
        fs::write(&trashed, b"payload").unwrap();
        fs::write(&original, b"existing").unwrap(); // 原址已存在

        write_ledger(&ledger, &[entry_with_trashed("r1", &original, &trashed)]);

        let report = undo_from_ledger(&ledger, "r1");
        assert_eq!(report.restored_count(), 0, "原址占用不放回");
        assert_eq!(report.skipped_count(), 1, "计为跳过而非失败");
        assert_eq!(fs::read(&original).unwrap(), b"existing", "原址文件不被覆盖");
    }

    #[test]
    fn undo_empty_ledger_returns_empty() {
        let dir = tempdir().unwrap();
        let ledger = dir.path().join("history.jsonl"); // 不存在
        let report = undo_from_ledger(&ledger, "r1");
        assert!(report.outcomes.is_empty());
    }
}
