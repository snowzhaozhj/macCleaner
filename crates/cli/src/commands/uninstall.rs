use crate::{Cli, Commands};
use mc_core::app_resolver::AppResolver;
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, SafetyLevel, ScanItem};
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
                eprintln!("\r已清理 {} 个文件，释放 {}", count, format_size(freed, DECIMAL));
            }
            _ => {}
        }
    }
}

pub fn run(cli: &Cli) -> Result<()> {
    let search = match &cli.command {
        Some(Commands::Uninstall { search }) => search.clone(),
        _ => None,
    };

    let mut apps = AppResolver::list_apps();

    if apps.is_empty() {
        println!("未发现已安装的应用。");
        return Ok(());
    }

    if let Some(ref query) = search {
        apps.retain(|app| {
            app.name.to_lowercase().contains(&query.to_lowercase())
                || app
                    .bundle_id
                    .as_ref()
                    .is_some_and(|b| b.to_lowercase().contains(&query.to_lowercase()))
        });
    }

    if apps.is_empty() {
        println!("未找到匹配的应用。");
        return Ok(());
    }

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&apps)?);
        return Ok(());
    }

    println!("已安装的应用:\n");
    for (i, app) in apps.iter().enumerate() {
        println!(
            "  {}. {} ({}) — {}",
            i + 1,
            app.name,
            app.bundle_id.as_deref().unwrap_or("unknown"),
            format_size(app.size, DECIMAL),
        );
    }

    eprint!("\n请输入要卸载的应用编号（或 q 取消）: ");
    io::stderr().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.eq_ignore_ascii_case("q") || input.is_empty() {
        println!("已取消。");
        return Ok(());
    }

    let index: usize = input.parse::<usize>().map_err(|_| anyhow::anyhow!("无效编号"))?;
    if index == 0 || index > apps.len() {
        anyhow::bail!("编号超出范围");
    }

    let app = &apps[index - 1];
    let mut items_to_clean: Vec<ScanItem> = Vec::new();

    items_to_clean.push(ScanItem::new(
        app.path.clone(),
        app.size,
        SafetyLevel::Safe,
        "Application".to_string(),
    ));

    if let Some(ref bundle_id) = app.bundle_id {
        let leftovers = AppResolver::find_leftovers(bundle_id);
        items_to_clean.extend(leftovers);
    }

    println!("\n将要卸载 {}:\n", app.name);
    let mut total_size: u64 = 0;
    for item in &items_to_clean {
        println!("  {} ({})", item.path.display(), format_size(item.size, DECIMAL));
        // 展示可能含用户数据的残留项的证据文案，避免"看不到依据却删除"（D3）。
        if !item.impact.trim().is_empty() {
            println!("    ⚠ {}", item.impact);
        }
        if !item.recovery.trim().is_empty() {
            println!("    ↩ {}", item.recovery);
        }
        total_size += item.size;
    }
    println!("\n  总计: {} 个文件/目录, {}", items_to_clean.len(), format_size(total_size, DECIMAL));

    if cli.dry_run {
        return Ok(());
    }

    if !cli.yes {
        eprint!("\n确认卸载？[y/N] ");
        io::stderr().flush()?;
        let mut confirm = String::new();
        io::stdin().read_line(&mut confirm)?;
        if !confirm.trim().eq_ignore_ascii_case("y") {
            println!("已取消。");
            return Ok(());
        }
    }

    let refs: Vec<&ScanItem> = items_to_clean.iter().collect();
    let mode = if cli.permanent { DeleteMode::Permanent } else { DeleteMode::Trash };
    let reporter = CliReporter;
    let report = Engine::clean(&refs, mode, &reporter)?;

    if report.failure_count > 0 {
        eprintln!("{} 个项目清理失败。", report.failure_count);
    }

    Ok(())
}
