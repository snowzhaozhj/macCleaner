use crate::{Cli, Commands};
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, SafetyLevel, ScanItem, ScanResult};
use mc_core::platform;
use mc_core::progress::{ProgressEvent, ProgressReporter};

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// CLI 进度上报器。`skipped` 收集扫描期间因权限跳过的路径（#23），扫描后单列展示。
#[derive(Default)]
struct CliReporter {
    skipped: Mutex<BTreeSet<PathBuf>>,
}

impl ProgressReporter for CliReporter {
    fn on_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Scanning { path } => {
                eprint!("\r扫描中: {} ", path.display());
            }
            ProgressEvent::SkippedNoPermission { path } => {
                if let Ok(mut s) = self.skipped.lock() {
                    s.insert(path);
                }
            }
            ProgressEvent::Found { category, path, size, .. } => {
                eprintln!("\r  发现 {} — {} ({})", category, path.display(), format_size(size, DECIMAL));
            }
            ProgressEvent::Complete => {
                eprintln!();
            }
            ProgressEvent::CleaningFile { path } => {
                eprint!("\r清理中: {} ", path.display());
            }
            ProgressEvent::CleaningDone { freed, count, .. } => {
                eprintln!("\r已清理 {} 个项目，释放 {}", count, format_size(freed, DECIMAL));
            }
            _ => {}
        }
    }
}

pub fn run(cli: &Cli) -> Result<()> {
    let path = match &cli.command {
        Some(Commands::Purge { path }) => path
            .as_ref().map_or_else(platform::get_home_dir, PathBuf::from),
        _ => platform::get_home_dir(),
    };

    if !path.exists() {
        anyhow::bail!("路径不存在: {}", path.display());
    }

    let reporter = CliReporter::default();
    eprintln!("正在扫描 {} 中的开发产物...\n", path.display());

    let result = Engine::scan_purge(&path, &reporter)?;

    // 扫描期间因权限跳过的路径单列展示，引导 mc doctor（#23）。
    print_skipped(&reporter);

    if result.file_count == 0 {
        println!("未发现开发产物。");
        return Ok(());
    }

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        if cli.dry_run {
            return Ok(());
        }
    } else {
        print_purge_summary(&result);
    }

    if cli.dry_run {
        return Ok(());
    }

    let items: Vec<_> = if cli.yes {
        result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.selected)
            .collect()
    } else {
        let selected: Vec<_> = result.selected_items();
        let selected_size: u64 = selected.iter().map(|i| i.size).sum();
        eprint!(
            "\n确认清理 {} 个开发产物目录，释放 {}？[y/N] ",
            selected.len(),
            format_size(selected_size, DECIMAL),
        );
        io::stderr().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("已取消。");
            return Ok(());
        }
        selected
    };

    if items.is_empty() {
        println!("没有选中的项目。");
        return Ok(());
    }

    let mode = if cli.permanent { DeleteMode::Permanent } else { DeleteMode::Trash };
    let report = Engine::clean(&items, mode, &reporter)?;

    // 写入只读账本（#24）：优雅降级，失败不影响清理结果
    super::history::record(mc_core::history::HistoryCommand::Purge, &items, &report);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.failure_count > 0 {
        eprintln!("{} 个项目清理失败。", report.failure_count);
    }

    Ok(())
}

/// 展示「跳过（需授权）」区：列出扫描时因权限读不到的路径并引导 mc doctor。
/// 无跳过则完全静默。
fn print_skipped(reporter: &CliReporter) {
    let Ok(skipped) = reporter.skipped.lock() else {
        return;
    };
    if skipped.is_empty() {
        return;
    }
    eprintln!("\n跳过（需授权）— {} 个路径因权限未能读取：", skipped.len());
    for path in skipped.iter() {
        eprintln!("  {}", path.display());
    }
    eprintln!("运行 mc doctor 查看磁盘访问权限与授权引导。\n");
}

fn print_purge_summary(result: &ScanResult) {
    println!("发现的开发产物:\n");
    for cat in &result.categories {
        println!("  {} ({} 个项目, {}):", cat.name, cat.file_count, format_size(cat.total_size, DECIMAL));
        for item in &cat.items {
            println!("    {}", format_item_line(item));
        }
    }
    println!(
        "\n  总计: {} 个项目, {}",
        result.file_count,
        format_size(result.total_size, DECIMAL),
    );
}

/// 单项展示：颜色标记 + 路径 + 大小 + 恢复方式（若有）。恢复文案让"删了怎么拿回来"可见。
fn format_item_line(item: &ScanItem) -> String {
    let safety_indicator = match item.safety {
        SafetyLevel::Safe => "\u{1f7e2}",
        SafetyLevel::Moderate => "\u{1f7e1}",
        SafetyLevel::Risky => "\u{1f534}",
    };
    let base = format!(
        "{} {} — {}",
        safety_indicator,
        item.path.display(),
        format_size(item.size, DECIMAL),
    );
    if item.recovery.trim().is_empty() {
        base
    } else {
        format!("{base}  · {}", item.recovery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn format_item_line_appends_recovery_when_present() {
        let item = ScanItem::new(PathBuf::from("/p/node_modules"), 1024, SafetyLevel::Moderate, "Node.js".into())
            .with_evidence("依赖被清空".into(), "运行 npm install".into());
        let line = format_item_line(&item);
        assert!(line.contains("/p/node_modules"), "应含路径");
        assert!(line.contains("· 运行 npm install"), "应追加 recovery 文案");
        assert!(line.contains('\u{1f7e1}'), "Moderate 应用黄色标记");
    }

    #[test]
    fn format_item_line_omits_recovery_when_empty() {
        let item = ScanItem::new(PathBuf::from("/app"), 512, SafetyLevel::Safe, "x".into());
        let line = format_item_line(&item);
        assert!(!line.contains('·'), "空 recovery 不应出现分隔符");
    }
}
