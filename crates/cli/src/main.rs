use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mc", about = "macCleaner — 快速、安全的 Mac 清理工具", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// 不实际删除，只展示将要清理的内容
    #[arg(long, global = true, alias = "preview")]
    pub dry_run: bool,

    /// 跳过确认，直接执行（仅 safe 级别）
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// 永久删除（不移到废纸篓）
    #[arg(long, global = true)]
    pub permanent: bool,

    /// JSON 格式输出
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 清理系统缓存、日志和临时文件
    Clean,
    /// 卸载应用并清理关联文件
    Uninstall {
        /// 搜索关键词
        #[arg(long)]
        search: Option<String>,
    },
    /// 分析磁盘用量
    Analyze {
        /// 分析路径（默认 ~/）
        path: Option<String>,
        /// 大文件阈值（MB，默认 100）
        #[arg(long, default_value = "100")]
        threshold: u64,
    },
    /// `清理开发产物（node_modules`, target 等）
    Purge {
        /// 扫描路径（默认 ~/）
        path: Option<String>,
    },
    /// 查看清理历史（上次清理以来的回收趋势）
    History,
    /// 从废纸篓放回上次清理的项（撤销上一次 clean/purge）
    Undo {
        /// 指定要撤销的清理记录 run-id（默认取最近一条可恢复的记录）
        run_id: Option<String>,
    },
    /// 诊断磁盘访问权限（检查 Full Disk Access，只读）
    Doctor,
    /// 扫描孤儿残留（父应用已卸载但 ~/Library 残留仍在，反向卸载）
    Orphans,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        None => mc_tui::run()?,
        Some(Commands::Clean) => commands::clean::run(&cli)?,
        Some(Commands::Uninstall { .. }) => commands::uninstall::run(&cli)?,
        Some(Commands::Analyze { .. }) => commands::analyze::run(&cli)?,
        Some(Commands::Purge { .. }) => commands::purge::run(&cli)?,
        Some(Commands::History) => commands::history::run(&cli)?,
        Some(Commands::Undo { ref run_id }) => commands::undo::run(&cli, run_id.as_deref())?,
        Some(Commands::Doctor) => commands::doctor::run(&cli)?,
        Some(Commands::Orphans) => commands::orphans::run(&cli)?,
    }

    Ok(())
}

mod commands;
