//! `mc history` 子命令 + clean/purge 完成后写账本的薄封装（issue #24）。
//!
//! 展示是纯读：把 `~/.local/state/mc/history.jsonl` 逐行读回，按时间列出每次清理的
//! 「多久以前 / 类型 / 释放量 / 条数」，末尾给累计。写账本的实际序列化在 `mc_core::history`，
//! 这里只负责"拿到清理报告后追加一条 + 失败优雅降级"。

use crate::Cli;
use mc_core::history::{self, HistoryCommand};
use mc_core::models::{CleanReport, ScanItem};

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::time::{SystemTime, UNIX_EPOCH};

/// clean/purge 成功清理后调用：构建账本条目并追加写入。
///
/// **优雅降级**：写失败只记 warn，绝不返回 Err、绝不中断清理主流程（账本是旁路观测，
/// 不是清理的一部分）。无成功项时不写（避免空记录污染账本）。实际写入逻辑在
/// `mc_core::history::record_run`（CLI/GUI 共享真源）；CLI 不需要它回传的 `run_id`，丢弃即可。
pub fn record(command: HistoryCommand, items: &[&ScanItem], report: &CleanReport) {
    let _ = history::record_run(command, items, report);
}

pub fn run(cli: &Cli) -> Result<()> {
    let path = history::default_path();
    let entries = history::load(&path);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("暂无清理记录。");
        println!("运行 mc clean 或 mc purge 清理后，回收趋势会在此累计。");
        return Ok(());
    }

    println!("清理账本（{}）\n", path.display());

    let now = now_unix_secs();
    let mut total_freed: u64 = 0;
    let mut total_count: usize = 0;
    for entry in &entries {
        total_freed += entry.freed;
        total_count += entry.count;
        println!(
            "  {:<10} {:<7} 释放 {:<10} {} 项",
            humanize_age(now.saturating_sub(entry.timestamp)),
            entry.command.label(),
            format_size(entry.freed, DECIMAL),
            entry.count,
        );
    }

    println!(
        "\n  累计：释放 {}，共 {} 项，{} 次清理",
        format_size(total_freed, DECIMAL),
        total_count,
        entries.len(),
    );

    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 把"多少秒以前"换算成克制的中文相对时间。纯算术，不引时区/日历依赖
/// （见 `history::HistoryEntry::timestamp` 的取舍说明）。
fn humanize_age(secs_ago: u64) -> String {
    match secs_ago {
        0..=59 => "刚刚".to_string(),
        60..=3599 => format!("{} 分钟前", secs_ago / 60),
        3600..=86_399 => format!("{} 小时前", secs_ago / 3600),
        _ => format!("{} 天前", secs_ago / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_age_buckets() {
        assert_eq!(humanize_age(0), "刚刚");
        assert_eq!(humanize_age(59), "刚刚");
        assert_eq!(humanize_age(60), "1 分钟前");
        assert_eq!(humanize_age(3599), "59 分钟前");
        assert_eq!(humanize_age(3600), "1 小时前");
        assert_eq!(humanize_age(86_400), "1 天前");
        assert_eq!(humanize_age(200_000), "2 天前");
    }
}
