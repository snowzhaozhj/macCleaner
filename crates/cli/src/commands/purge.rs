use crate::{Cli, Commands};
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, SafetyLevel, ScanResult};
use mc_core::platform;
use mc_core::progress::{ProgressEvent, ProgressReporter};

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::io::{self, Write};
use std::path::PathBuf;

struct CliReporter;

impl ProgressReporter for CliReporter {
    fn on_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Scanning { path } => {
                eprint!("\r扫描中: {} ", path.display());
            }
            ProgressEvent::Found { category, path, size } => {
                eprintln!("\r  发现 {} — {} ({})", category, path.display(), format_size(size, DECIMAL));
            }
            ProgressEvent::Complete => {
                eprintln!();
            }
            ProgressEvent::CleaningFile { path } => {
                eprint!("\r清理中: {} ", path.display());
            }
            ProgressEvent::CleaningDone { freed, count } => {
                eprintln!("\r已清理 {} 个项目，释放 {}", count, format_size(freed, DECIMAL));
            }
            _ => {}
        }
    }
}

pub fn run(cli: &Cli) -> Result<()> {
    let path = match &cli.command {
        Some(Commands::Purge { path }) => path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| platform::get_home_dir()),
        _ => platform::get_home_dir(),
    };

    if !path.exists() {
        anyhow::bail!("路径不存在: {}", path.display());
    }

    let reporter = CliReporter;
    eprintln!("正在扫描 {} 中的开发产物...\n", path.display());

    let result = Engine::scan_purge(&path, &reporter)?;

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
            .filter(|i| i.selected && i.safety == SafetyLevel::Safe)
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

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.failure_count > 0 {
        eprintln!("{} 个项目清理失败。", report.failure_count);
    }

    Ok(())
}

fn print_purge_summary(result: &ScanResult) {
    println!("发现的开发产物:\n");
    for cat in &result.categories {
        println!("  {} ({} 个项目, {}):", cat.name, cat.file_count, format_size(cat.total_size, DECIMAL));
        for item in &cat.items {
            let safety_indicator = match item.safety {
                SafetyLevel::Safe => "\u{1f7e2}",
                SafetyLevel::Moderate => "\u{1f7e1}",
                SafetyLevel::Risky => "\u{1f534}",
            };
            println!(
                "    {} {} — {}",
                safety_indicator,
                item.path.display(),
                format_size(item.size, DECIMAL),
            );
        }
    }
    println!(
        "\n  总计: {} 个项目, {}",
        result.file_count,
        format_size(result.total_size, DECIMAL),
    );
}
