//! `mc doctor` 子命令（issue #23）：只读诊断磁盘访问权限。
//!
//! 探测一组需要 Full Disk Access 的标准路径，区分可读 / 缺授权 / 不存在，
//! 给出克制的授权引导。纯只读——绝不修改任何系统状态。

use crate::Cli;
use mc_core::doctor::{self, PathStatus};

use anyhow::Result;

pub fn run(cli: &Cli) -> Result<()> {
    let paths = doctor::standard_fda_paths();
    let results = doctor::probe_all(&paths);

    if cli.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    let denied = results
        .iter()
        .filter(|r| r.status == PathStatus::NoPermission)
        .count();

    println!("磁盘访问诊断\n");

    // 整体结论：有权限被拒 → 未授权；否则视为已授权（或这些目录本就不存在）。
    if denied == 0 {
        println!("  Full Disk Access：未发现被拒的路径（mc 可读取所检查区域）。\n");
    } else {
        println!("  Full Disk Access：{denied} 个路径当前不可读（需授权）。\n");
    }

    for r in &results {
        let home = mc_core::platform::get_home_dir();
        let shown = r
            .path
            .strip_prefix(&home)
            .map_or_else(|_| r.path.display().to_string(), |rel| format!("~/{}", rel.display()));
        let (mark, note) = match &r.status {
            PathStatus::Readable => ("[可读]", String::new()),
            PathStatus::NoPermission => ("[需授权]", String::new()),
            PathStatus::Missing => ("[不存在]", String::new()),
            PathStatus::Error(e) => ("[读取错误]", format!("（{e}）")),
        };
        println!("  {mark:<9} {shown}{note}");
    }

    // 授权引导：仅在确有被拒路径时给出，文案克制不吓唬。
    if denied > 0 {
        println!("\n如需让 mc 读取上述区域，可在");
        println!("  系统设置 > 隐私与安全性 > 完全磁盘访问权限");
        println!("中为你的终端（或 mc）开启访问后重试。");
        println!("mc doctor 只做只读诊断，不会修改任何系统设置。");
    }

    Ok(())
}
