//! xtask —— 仓库自动化任务。目前只有一个子命令：
//!
//! ```text
//! cargo run -p xtask -- gen-rules
//! ```
//!
//! `gen-rules` 把 `mc_core::rules::all_rules()` 投影成人类可读的 `RULES.md`
//! （规则透明度页）。这是**纯数据投影**——不改安全模型、不新增 `mc` 子命令，
//! 只把编译进二进制的清理规则表导出成 Markdown，供审计与信任建设。
//!
//! CI 有一道「漂移门禁」：跑本命令后 `git diff --exit-code RULES.md`，
//! 强制 `RULES.md` 与规则源始终同步。因此生成必须**确定性**：规则顺序来自
//! TOML 源（稳定），且所有 home 相对路径统一还原为 `~`（消除跨机器 home 差异）。

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mc_core::models::SafetyLevel;
use mc_core::platform::get_home_dir;
use mc_core::rules::{clean_rules, purge_rules, CleanRule, PathPattern, RootMarker};

fn main() -> ExitCode {
    let cmd = std::env::args().nth(1);
    match cmd.as_deref() {
        Some("gen-rules") => match gen_rules() {
            Ok(path) => {
                println!("已生成 {}", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("gen-rules 失败: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            if let Some(c) = other {
                eprintln!("未知子命令: {c}");
            }
            eprintln!("用法: cargo run -p xtask -- gen-rules");
            ExitCode::FAILURE
        }
    }
}

/// 生成 `RULES.md` 到仓库根，返回写入路径。
fn gen_rules() -> std::io::Result<PathBuf> {
    let home = get_home_dir();
    let markdown = render(&home);
    let out = repo_root().join("RULES.md");
    std::fs::write(&out, markdown)?;
    Ok(out)
}

/// 仓库根 = xtask crate 目录的父级（`xtask/` 位于仓库根下）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask 应位于仓库根的子目录")
        .to_path_buf()
}

fn render(home: &Path) -> String {
    let clean = clean_rules();
    let purge = purge_rules();
    let total = clean.len() + purge.len();

    let mut safe = 0;
    let mut moderate = 0;
    let mut risky = 0;
    for r in clean.iter().chain(purge.iter()) {
        match r.safety {
            SafetyLevel::Safe => safe += 1,
            SafetyLevel::Moderate => moderate += 1,
            SafetyLevel::Risky => risky += 1,
        }
    }

    let mut out = String::new();
    out.push_str("<!-- 本文件由 xtask 自动生成：`cargo run -p xtask -- gen-rules`。请勿手改；改规则请改 crates/core/src/*_rules.toml，再重跑生成。 -->\n\n");
    out.push_str("# 清理规则透明度\n\n");
    out.push_str(
        "本页把编译进 `mc` 二进制的**全部清理规则**逐条列出——路径模式、安全等级、删除影响、\
恢复方式、项目根守卫、分类。目的是让任何人无需读代码即可审计「这个工具到底会动哪些文件、\
为什么安全」。内容由源规则表机械投影而成，与二进制行为一一对应（CI 有漂移门禁保证同步）。\n\n",
    );
    out.push_str("安全等级语义（详见 `crates/core/src/models.rs` 的 `SafetyLevel` 文档注释）：\n\n");
    out.push_str("- **Safe**：零数据丢失，下次需要时自动透明补回（共享/下载缓存、IDE 索引等）。默认勾选。\n");
    out.push_str("- **Moderate**：零数据丢失，但需用户主动重建一次（`node_modules`、`target`、`DerivedData` 等）。默认勾选。\n");
    out.push_str("- **Risky**：可能丢失不可再生数据或有价值状态（Docker 命名卷、Xcode Archives、装好环境的 AVD）。默认不勾选，删除需在 TUI 输入 `delete` 二次确认。\n\n");
    let _ = writeln!(
        out,
        "> 合计 **{total}** 条规则：Safe {safe} · Moderate {moderate} · Risky {risky}。删除默认移入废纸篓（可恢复）。安全边界见 [SECURITY.md](SECURITY.md)。\n",
    );

    out.push_str("## 系统缓存与日志（Clean）\n\n");
    out.push_str("`mc clean` 使用的规则：按精确路径匹配系统缓存、日志、临时文件。\n\n");
    push_table(&mut out, &clean, home);

    out.push_str("\n## 开发产物（Purge）\n\n");
    out.push_str("`mc purge <dir>` 使用的规则：按目录名剪枝匹配开发依赖与构建产物，命中需满足「项目根守卫」以消除误报。\n\n");
    push_table(&mut out, &purge, home);

    out
}

fn push_table(out: &mut String, rules: &[CleanRule], home: &Path) {
    out.push_str("| 规则 | 路径模式 | 安全等级 | 默认预选 | 影响 | 恢复 | 项目根守卫 | 分类 |\n");
    out.push_str("| --- | --- | --- | :---: | --- | --- | --- | --- |\n");
    for r in rules {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            cell(&r.name),
            fmt_patterns(r, home),
            fmt_safety(r.safety),
            if r.preselect { "是" } else { "否" },
            cell(&r.impact),
            cell(&r.recovery),
            fmt_markers(&r.root_markers),
            cell(&r.category),
        );
    }
}

fn fmt_safety(s: SafetyLevel) -> &'static str {
    match s {
        SafetyLevel::Safe => "Safe",
        SafetyLevel::Moderate => "Moderate",
        SafetyLevel::Risky => "Risky",
    }
}

/// 格式化路径模式。home 相对的 `Exact` 还原成 `~/…`（保证跨机器确定性）；
/// 真正的绝对路径（如 `/private/var/…`）原样保留；`DirName` 显示为 `name/`。
fn fmt_patterns(rule: &CleanRule, home: &Path) -> String {
    rule.patterns
        .iter()
        .map(|p| match p {
            PathPattern::Exact(path) => match path.strip_prefix(home) {
                Ok(rel) if rel.as_os_str().is_empty() => "`~`".to_string(),
                Ok(rel) => format!("`~/{}`", rel.display()),
                Err(_) => format!("`{}`", path.display()),
            },
            PathPattern::DirName(name) => format!("`{name}/`"),
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

fn fmt_markers(markers: &[RootMarker]) -> String {
    if markers.is_empty() {
        return "—".to_string();
    }
    markers
        .iter()
        .map(|m| match m {
            RootMarker::Sibling(name) => format!("旁有 `{name}`"),
            RootMarker::Inside(name) => format!("内含 `{name}`"),
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

/// 转义表格单元格：`|` 会破坏列，换行会破坏行——分别转义/替换为 `<br>`。
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', "<br>")
}
