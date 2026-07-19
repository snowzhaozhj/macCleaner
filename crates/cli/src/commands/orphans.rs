//! `mc orphans` 子命令：反向卸载——扫描父 App 已不存在的孤儿残留（issue #27 / beat-mole #5）。
//!
//! 与 `mc uninstall`（正向：选一个已装 App 卸载它的残留）互补：本命令全局扫描 `~/Library`，
//! 列出没有主人的残留。**孤儿一律不预选**（`AppResolver::scan_orphans` 决定）——App 已卸载但
//! 用户可能故意保留数据，故不默认删、不 `--yes` 自动删，须用户显式勾选要删的项。

use crate::Cli;
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, ScanItem};
use mc_core::progress::{ProgressEvent, ProgressReporter};

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::io::{self, Write};

struct CliReporter;

impl ProgressReporter for CliReporter {
    fn on_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::CleaningFile { path } => {
                eprint!("\r删除中: {} ", path.display());
            }
            ProgressEvent::CleaningDone { freed, count, .. } => {
                eprintln!("\r已清理 {} 个项目，释放 {}", count, format_size(freed, DECIMAL));
            }
            _ => {}
        }
    }
}

/// 解析用户对孤儿列表的选择输入（纯函数，便于单测）。
///
/// - `q` / 空 → `None`（取消）。
/// - `a` → 全选（返回 `0..len` 全部下标）。
/// - 逗号/空格分隔的编号与范围（`1,3 5-7`）→ 对应 0-based 下标集合（去重、升序）。
/// - 任一编号越界（0 或 > len）或非数字 → `Err`。
///
/// 返回 0-based 下标，`Ok(Some(vec))`；空选择（如仅空白）视为取消 `Ok(None)`。
pub fn parse_selection(input: &str, len: usize) -> Result<Option<Vec<usize>>> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("q") {
        return Ok(None);
    }
    if trimmed.eq_ignore_ascii_case("a") {
        return Ok(Some((0..len).collect()));
    }

    let mut indices: Vec<usize> = Vec::new();
    for token in trimmed.split([',', ' ']).filter(|t| !t.is_empty()) {
        if let Some((lo, hi)) = token.split_once('-') {
            let lo: usize = lo.trim().parse().map_err(|_| anyhow::anyhow!("无效范围: {token}"))?;
            let hi: usize = hi.trim().parse().map_err(|_| anyhow::anyhow!("无效范围: {token}"))?;
            if lo == 0 || hi == 0 || lo > len || hi > len || lo > hi {
                anyhow::bail!("范围超出或反向: {token}（有效 1-{len}）");
            }
            for n in lo..=hi {
                indices.push(n - 1);
            }
        } else {
            let n: usize = token.parse().map_err(|_| anyhow::anyhow!("无效编号: {token}"))?;
            if n == 0 || n > len {
                anyhow::bail!("编号超出范围: {n}（有效 1-{len}）");
            }
            indices.push(n - 1);
        }
    }

    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        return Ok(None);
    }
    Ok(Some(indices))
}

pub fn run(cli: &Cli) -> Result<()> {
    let orphans = Engine::scan_orphans();

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&orphans)?);
        return Ok(());
    }

    if orphans.is_empty() {
        println!("未发现孤儿残留（父应用已卸载但残留仍在的项）。");
        return Ok(());
    }

    println!("发现 {} 个孤儿残留（父应用已卸载）:\n", orphans.len());
    let mut total_size: u64 = 0;
    for (i, item) in orphans.iter().enumerate() {
        println!(
            "  {}. {} ({}) — {}",
            i + 1,
            item.path.display(),
            item.category,
            format_size(item.size, DECIMAL),
        );
        // 展示可能含用户数据的残留项的证据文案（D3 同源约束）。
        if !item.impact.trim().is_empty() {
            println!("     ⚠ {}", item.impact);
        }
        if !item.recovery.trim().is_empty() {
            println!("     ↩ {}", item.recovery);
        }
        total_size += item.size;
    }
    println!(
        "\n  总计: {} 项, {}",
        orphans.len(),
        format_size(total_size, DECIMAL)
    );

    if cli.dry_run {
        return Ok(());
    }

    // 孤儿一律不预选（KTD2）：--yes 也不自动删，明确提示须显式勾选，避免"静默无操作"的困惑。
    if cli.yes {
        println!(
            "\n孤儿残留默认不勾选（父应用已卸载，可能是你有意保留的数据）。\n\
             --yes 不会自动删除孤儿；请去掉 --yes 交互指定要删的编号。"
        );
        return Ok(());
    }

    eprint!(
        "\n请输入要删除的编号（如 1,3 或 2-5；a=全部，q=取消）: "
    );
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let Some(indices) = parse_selection(&input, orphans.len())? else {
        println!("已取消。");
        return Ok(());
    };

    let selected: Vec<&ScanItem> = indices.iter().map(|&i| &orphans[i]).collect();
    let mode = if cli.permanent { DeleteMode::Permanent } else { DeleteMode::Trash };
    let reporter = CliReporter;
    let report = Engine::clean(&selected, mode, &reporter)?;

    if report.failure_count > 0 {
        eprintln!("{} 个项目清理失败。", report.failure_count);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_selection;

    #[test]
    fn empty_and_q_cancel() {
        assert!(parse_selection("", 5).unwrap().is_none());
        assert!(parse_selection("   ", 5).unwrap().is_none());
        assert!(parse_selection("q", 5).unwrap().is_none());
        assert!(parse_selection("Q", 5).unwrap().is_none());
    }

    #[test]
    fn a_selects_all() {
        assert_eq!(parse_selection("a", 3).unwrap().unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn numbers_map_to_zero_based_indices() {
        assert_eq!(parse_selection("1,3", 3).unwrap().unwrap(), vec![0, 2]);
        // 空格分隔、乱序、重复 → 去重升序。
        assert_eq!(parse_selection("3 1 1", 3).unwrap().unwrap(), vec![0, 2]);
    }

    #[test]
    fn range_expands_inclusive() {
        assert_eq!(parse_selection("2-4", 5).unwrap().unwrap(), vec![1, 2, 3]);
        // 范围与单号混用、去重。
        assert_eq!(parse_selection("1,2-3,3", 5).unwrap().unwrap(), vec![0, 1, 2]);
    }

    #[test]
    fn out_of_range_errors() {
        assert!(parse_selection("0", 3).is_err());
        assert!(parse_selection("4", 3).is_err());
        assert!(parse_selection("2-9", 3).is_err());
        assert!(parse_selection("3-1", 3).is_err()); // 反向
        assert!(parse_selection("x", 3).is_err());
    }
}
