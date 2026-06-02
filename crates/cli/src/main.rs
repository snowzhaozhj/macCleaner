use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mc", about = "macCleaner — 快速、安全的 Mac 清理工具", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

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
    /// 清理开发产物（node_modules, target 等）
    Purge {
        /// 扫描路径（默认 ~/）
        path: Option<String>,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Clean => commands::clean::run(&cli)?,
        Commands::Uninstall { .. } => commands::uninstall::run(&cli)?,
        Commands::Analyze { .. } => commands::analyze::run(&cli)?,
        Commands::Purge { .. } => commands::purge::run(&cli)?,
    }

    Ok(())
}

mod commands;
