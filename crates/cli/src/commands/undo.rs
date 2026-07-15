//! `mc undo` 子命令：把上一次 `clean`/`purge` 移入废纸篓的项确定性放回原处。
//!
//! 数据源是清理账本里的 `restorable` 映射（原始路径 → 废纸篓落点，见 `mc_core::history`）。
//! 恢复动作与安全护栏在 `mc_core::restore`；本文件只负责"选哪条账本 + 渲染结果 + 无映射时降级"。
//!
//! 降级：账本条目无 `restorable` 映射（本功能上线前的旧记录，或落点未能捕获）时，
//! 无法确定性放回——提示用户打开 `~/.Trash` 用 Finder 原生「放回原处」。

use crate::Cli;
use mc_core::history::{self, HistoryEntry};
use mc_core::restore::{self, RestoreStatus};

use anyhow::Result;

/// 选出要恢复的账本条目。
///
/// - 给定 `run_id`：精确匹配该次运行（即便它无可恢复映射，也交由调用方给出降级提示）。
/// - 未给定：取**最近一条含可恢复映射**的条目（跳过无映射的旧记录，避免"undo 却说没东西可恢复"）。
#[must_use]
pub fn select_entry<'a>(
    entries: &'a [HistoryEntry],
    run_id: Option<&str>,
) -> Option<&'a HistoryEntry> {
    match run_id {
        Some(id) => entries.iter().find(|e| e.run_id == id),
        None => entries.iter().rev().find(|e| !e.restorable.is_empty()),
    }
}

pub fn run(cli: &Cli, run_id: Option<&str>) -> Result<()> {
    let path = history::default_path();
    let entries = history::load(&path);

    let Some(entry) = select_entry(&entries, run_id) else {
        // 找不到目标条目：给定的 run-id 不存在，或账本里根本没有可恢复的记录。
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&restore::RestoreReport::default())?);
        } else if run_id.is_some() {
            println!("未找到该次清理记录（run-id 不存在）。");
        } else {
            println!("暂无可确定性恢复的清理记录。");
            print_finder_hint();
        }
        return Ok(());
    };

    if entry.restorable.is_empty() {
        // 命中了条目但它无落点映射（旧记录/未捕获）→ 降级到 Finder 放回。
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&restore::RestoreReport::default())?);
        } else {
            println!("该次清理（{}）无确定性落点记录，无法自动放回。", entry.command.label());
            print_finder_hint();
        }
        return Ok(());
    }

    let report = restore::restore(&entry.restorable, cli.dry_run);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render(&report, entry);
    Ok(())
}

/// 打印 Finder「放回原处」降级提示，附上废纸篓路径。
fn print_finder_hint() {
    let trash = mc_core::platform::trash_dir();
    println!("可打开废纸篓，用 Finder 的「放回原处」手动恢复：{}", trash.display());
}

fn render(report: &restore::RestoreReport, entry: &HistoryEntry) {
    let restored = report.restored_count();
    let skipped = report.skipped_count();
    let failed = report.failed_count();

    if report.dry_run {
        println!(
            "预览：将从废纸篓放回 {restored} 项（本次清理 {}，未实际移动文件）。",
            entry.command.label()
        );
    } else {
        println!("已从废纸篓放回 {restored} 项（本次清理 {}）。", entry.command.label());
    }

    // 只逐项列出未成功的项，帮助用户判断为何某些项没放回。
    for o in &report.outcomes {
        match o.status {
            RestoreStatus::Restored => {}
            RestoreStatus::SkippedTargetOccupied => {
                println!("  跳过（原址已存在，未覆盖）：{}", o.original.display());
            }
            RestoreStatus::SkippedTrashMissing => {
                println!("  跳过（废纸篓中已无此项）：{}", o.original.display());
            }
            RestoreStatus::Failed => {
                let reason = o.error.as_deref().unwrap_or("未知错误");
                println!("  失败：{}（{reason}）", o.original.display());
            }
        }
    }

    if skipped > 0 || failed > 0 {
        println!("\n共跳过 {skipped} 项，失败 {failed} 项。");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::history::{HistoryCommand, RestoreEntry};
    use std::path::PathBuf;

    fn entry(run_id: &str, restorable: Vec<&str>) -> HistoryEntry {
        HistoryEntry {
            run_id: run_id.into(),
            timestamp: 1,
            command: HistoryCommand::Clean,
            freed: 0,
            count: restorable.len(),
            categories: vec![],
            deleted_paths: vec![],
            restorable: restorable
                .into_iter()
                .map(|p| RestoreEntry {
                    original: PathBuf::from(p),
                    trashed_to: PathBuf::from(format!("/T/{p}")),
                })
                .collect(),
        }
    }

    #[test]
    fn select_none_picks_latest_with_mapping() {
        // 最后一条有映射 → 选它。
        let entries = vec![entry("r1", vec!["/a"]), entry("r2", vec!["/b"])];
        assert_eq!(select_entry(&entries, None).unwrap().run_id, "r2");
    }

    #[test]
    fn select_none_skips_trailing_entries_without_mapping() {
        // 最后一条无映射（旧记录），更早一条有 → 选更早那条有映射的。
        let entries = vec![entry("r1", vec!["/a"]), entry("r2", vec![])];
        assert_eq!(select_entry(&entries, None).unwrap().run_id, "r1");
    }

    #[test]
    fn select_none_returns_none_when_no_mapping_anywhere() {
        let entries = vec![entry("r1", vec![]), entry("r2", vec![])];
        assert!(select_entry(&entries, None).is_none());
    }

    #[test]
    fn select_by_run_id_hits_exact() {
        let entries = vec![entry("r1", vec!["/a"]), entry("r2", vec!["/b"])];
        assert_eq!(select_entry(&entries, Some("r1")).unwrap().run_id, "r1");
    }

    #[test]
    fn select_by_run_id_missing_returns_none() {
        let entries = vec![entry("r1", vec!["/a"])];
        assert!(select_entry(&entries, Some("nope")).is_none());
    }

    #[test]
    fn select_empty_ledger_returns_none() {
        let entries: Vec<HistoryEntry> = vec![];
        assert!(select_entry(&entries, None).is_none());
        assert!(select_entry(&entries, Some("r1")).is_none());
    }
}
