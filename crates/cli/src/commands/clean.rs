use crate::Cli;
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, SafetyLevel};
use mc_core::models::ScanResult;
use mc_core::platform;
use mc_core::progress::{ProgressEvent, ProgressReporter};

use anyhow::Result;
use humansize::{format_size, DECIMAL};
use std::io::{self, Write};

struct CliReporter;

impl ProgressReporter for CliReporter {
    fn on_event(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Scanning { path } => {
                eprint!("\r扫描中: {} ", path.display());
            }
            ProgressEvent::Found { .. } => {
                // 可用于更新进度条
            }
            ProgressEvent::CategoryDone {
                category,
                total_size,
                count,
            } => {
                eprintln!(
                    "\r  {} — {} 个文件, {}",
                    category,
                    count,
                    format_size(total_size, DECIMAL)
                );
            }
            ProgressEvent::Complete => {
                eprintln!();
            }
            ProgressEvent::CleaningFile { path } => {
                eprint!("\r清理中: {} ", path.display());
            }
            ProgressEvent::CleaningDone { freed, count } => {
                eprintln!(
                    "\r已清理 {} 个文件，释放 {}",
                    count,
                    format_size(freed, DECIMAL)
                );
            }
            ProgressEvent::Error(msg) => {
                eprintln!("错误: {}", msg);
            }
        }
    }
}

pub fn run(cli: &Cli) -> Result<()> {
    let reporter = CliReporter;

    // 1. 扫描
    eprintln!("正在扫描...\n");
    let result = Engine::scan_clean(&reporter)?;

    if result.file_count == 0 {
        println!("未发现可清理的文件。");
        return Ok(());
    }

    // 2. 展示结果
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        if cli.dry_run {
            return Ok(());
        }
    } else {
        print_summary(&result);
    }

    // 3. dry-run 模式：仅展示，不执行
    if cli.dry_run {
        return Ok(());
    }

    // 4. 确认（除非 --yes）
    let items: Vec<_> = if cli.yes {
        // --yes 仅清理 safe 级别的项目
        result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.selected && i.safety == SafetyLevel::Safe)
            .collect()
    } else {
        // 交互确认
        let selected: Vec<_> = result.selected_items();
        let selected_size: u64 = selected.iter().map(|i| i.size).sum();
        eprint!(
            "\n确认清理 {} 个文件，释放 {}？[y/N] ",
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

    // 5. 执行清理
    let mode = if cli.permanent {
        DeleteMode::Permanent
    } else {
        DeleteMode::Trash
    };
    let report = Engine::clean(&items, mode, &reporter)?;

    // 6. 展示报告
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if report.failure_count > 0 {
        eprintln!("{} 个文件清理失败。", report.failure_count);
    }

    // 7. 废纸篓提示（仅 Trash 模式，非 --yes）
    if mode == DeleteMode::Trash && !cli.yes {
        if let Ok(trash_size) = platform::get_trash_size() {
            if trash_size > 0 {
                eprint!(
                    "废纸篓当前占用 {}，是否清空？[y/N] ",
                    format_size(trash_size, DECIMAL)
                );
                io::stderr().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if input.trim().eq_ignore_ascii_case("y") {
                    platform::empty_trash()?;
                    eprintln!("废纸篓已清空。");
                }
            }
        }
    }

    Ok(())
}

fn print_summary(result: &ScanResult) {
    println!("扫描结果:\n");
    for cat in &result.categories {
        let safety_indicator = if cat.items.iter().all(|i| i.safety == SafetyLevel::Safe) {
            "\u{1f7e2}" // 绿色圆
        } else if cat.items.iter().any(|i| i.safety == SafetyLevel::Risky) {
            "\u{1f534}" // 红色圆
        } else {
            "\u{1f7e1}" // 黄色圆
        };
        println!(
            "  {} {} — {} 个文件, {}",
            safety_indicator,
            cat.name,
            cat.file_count,
            format_size(cat.total_size, DECIMAL),
        );
    }
    println!(
        "\n  总计: {} 个文件, {}",
        result.file_count,
        format_size(result.total_size, DECIMAL),
    );
}
