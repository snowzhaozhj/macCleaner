//! 只读清理账本（issue #24）。
//!
//! 每次 clean/purge 成功删除后，向 `~/.local/state/mc/history.jsonl` **追加**一行 JSON
//! （JSONL：一行一条，append 不覆盖），记录本次回收的时间/类型/各分类/总量/成功路径。
//! 数据源是清理报告里**只含成功项**的部分（`CleanReport` success 项 == `CleaningDone.deleted_paths`），
//! 故账本天然只记真正被删掉的东西。
//!
//! 设计约束：
//! - **零遥测纯本地**：只写用户自己机器上的 state 目录，不含任何网络。
//! - **优雅降级**：写账本失败绝不能让清理本身报错——调用方负责吞掉 `record` 的 Err 记 warn。
//! - **不做 undo**：这是只读账本，恢复靠废纸篓；undo 是独立命题（Trash 非事务日志）。
//! - **无重依赖**：run-id/时间戳用 `std::time`，序列化复用已有的 `serde`/`serde_json`。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::models::{CleanReport, ScanItem};

/// 账本记录的命令类型。序列化为小写字符串（`"clean"` / `"purge"`），便于人读与外部消费。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryCommand {
    Clean,
    Purge,
}

impl HistoryCommand {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            HistoryCommand::Clean => "clean",
            HistoryCommand::Purge => "purge",
        }
    }
}

/// 单个分类在一次清理里的回收小结。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CategoryStat {
    pub name: String,
    pub size: u64,
    pub count: usize,
}

/// 一条可确定性恢复的映射：原始路径 → 移入废纸篓后的落点（含 inode 身份）。
///
/// 只有 Trash 删除且落点+inode 捕获成功的成功项才产生此映射（见 `cleaner::Cleaner::move_to_trash`）。
/// `mc undo` 据此把 `trashed_to` 放回 `original`，并用 `trashed_ino` 校验废纸篓里的名字仍是当初那个文件
/// （macOS 清空后复用名字，仅凭路径会误恢复无关同名文件）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreEntry {
    pub original: PathBuf,
    pub trashed_to: PathBuf,
    pub trashed_ino: u64,
}

/// 一次清理的账本条目（JSONL 中的一行）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// 本次运行的唯一标识（纳秒时间戳 + 进程号，纯本地防碰撞，无跨机语义）。
    pub run_id: String,
    /// Unix 秒时间戳（UTC）。展示端换算成"多久以前"，不在此处做日历运算以免引时区依赖。
    pub timestamp: u64,
    pub command: HistoryCommand,
    /// 本次成功释放的总字节数（== 各分类 size 之和 == `CleanReport::total_freed`）。
    pub freed: u64,
    /// 本次成功删除的条目数。
    pub count: usize,
    pub categories: Vec<CategoryStat>,
    /// 成功删除的路径列表（只含成功项，与剪树安全数据源同源）。
    pub deleted_paths: Vec<PathBuf>,
    /// 可确定性恢复的映射（原始路径 → 废纸篓落点）。只含 Trash 删除且捕获到落点的成功项，
    /// 故通常是 `deleted_paths` 的子集。`#[serde(default)]` 保证 #24 已写入的旧账本行
    /// （无此字段）仍能加载——旧行 `restorable` 为空，`mc undo` 对其降级到 Finder 放回。
    #[serde(default)]
    pub restorable: Vec<RestoreEntry>,
}

impl HistoryEntry {
    /// 从"选中项 + 清理报告"构建账本条目。
    ///
    /// `items` 是提交给 `Engine::clean` 的项（带 category/size），`report` 标注每项成败。
    /// 只把**成功删除**的项计入：按路径把成功项回连到其分类，聚合出各分类的 size/count。
    /// 二者的成功集合一致（cleaner 的 `deleted_paths` 也走 `filter(success)`）。
    #[must_use]
    pub fn from_report(command: HistoryCommand, items: &[&ScanItem], report: &CleanReport) -> Self {
        use std::collections::BTreeMap;

        // 成功删除的路径集合（顺序保留用于 deleted_paths）。
        let deleted_paths: Vec<PathBuf> = report
            .cleaned
            .iter()
            .filter(|c| c.success)
            .map(|c| c.path.clone())
            .collect();
        let deleted_set: std::collections::HashSet<&Path> =
            deleted_paths.iter().map(PathBuf::as_path).collect();

        // 按分类聚合成功项（BTreeMap 使输出按分类名稳定排序）。
        let mut by_cat: BTreeMap<String, (u64, usize)> = BTreeMap::new();
        for item in items {
            if deleted_set.contains(item.path.as_path()) {
                let e = by_cat.entry(item.category.clone()).or_insert((0, 0));
                e.0 += item.size;
                e.1 += 1;
            }
        }
        let categories = by_cat
            .into_iter()
            .map(|(name, (size, count))| CategoryStat { name, size, count })
            .collect();

        // 可恢复映射：成功且捕获到废纸篓落点+inode 的项（Trash 删除专属；永久删除/捕获失败无落点）。
        let restorable: Vec<RestoreEntry> = report
            .cleaned
            .iter()
            .filter(|c| c.success)
            .filter_map(|c| {
                // 路径与 inode 必须成对；缺一即视为未捕获，不产生可恢复映射。
                match (c.trashed_to.clone(), c.trashed_ino) {
                    (Some(trashed_to), Some(trashed_ino)) => Some(RestoreEntry {
                        original: c.path.clone(),
                        trashed_to,
                        trashed_ino,
                    }),
                    _ => None,
                }
            })
            .collect();

        Self {
            run_id: gen_run_id(),
            timestamp: now_unix_secs(),
            command,
            freed: report.total_freed,
            count: report.success_count,
            categories,
            deleted_paths,
            restorable,
        }
    }
}

/// 账本文件默认路径：`~/.local/state/mc/history.jsonl`。
///
/// 显式拼 `.local/state`（而非 `dirs::state_dir`）：后者在 macOS 上返回 `None`，
/// 而 issue 明确要求这个跨平台一致的落点。
#[must_use]
pub fn default_path() -> PathBuf {
    crate::platform::get_home_dir()
        .join(".local/state/mc/history.jsonl")
}

/// 选出要恢复的账本条目（CLI `mc undo` 与 GUI 撤销共享的单一真源）。
///
/// - 给定 `run_id`：精确匹配该次运行（即便它无可恢复映射，也交由调用方给出降级提示）。
///   GUI 回执撤销**必须**走这条：账本是 CLI/GUI 共享文件，只有按回执自身 `run_id` 精确命中
///   才能保证撤销的是"这张回执那次"，而非被终端 `mc clean` 或后续清理写入的更新条目劫持。
/// - 未给定：取**最近一条含可恢复映射**的条目（跳过无映射的旧记录，避免"undo 却说没东西可恢复"）。
///   CLI `mc undo`（无参）用此默认。
#[must_use]
pub fn select_entry<'a>(
    entries: &'a [HistoryEntry],
    run_id: Option<&str>,
) -> Option<&'a HistoryEntry> {
    match run_id {
        Some(id) => entries.iter().find(|e| e.run_id == id),
        None => entries.iter().rev().find(|e| !e.restorable.is_empty()),
    }
}

/// 追加一条账本记录（JSONL：序列化成一行 + 换行，`O_APPEND` 不覆盖既有内容）。
///
/// 失败即返回 Err（父目录建不出、无写权限等），由调用方优雅降级——**绝不 panic、
/// 也不让清理主流程失败**。
pub fn record(entry: &HistoryEntry, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// 成功清理后写账本（CLI `mc clean/purge` 与 GUI 共享的旁路写入真源）。
///
/// **旁路观测语义**：账本是清理的旁路记录，不是清理的一部分。
/// - 无成功项 → 不写、返回 `None`（避免空记录污染账本）。
/// - 写失败 → 只 `log::warn!`、返回 `None`，**绝不** panic、绝不让清理主流程失败。
///
/// 成功写入才返回 `Some(run_id)`——只有此时才存在可确定性撤销/恢复的账本条目。
/// CLI 忽略返回值（`let _ = …`）；GUI 用它作回执一键撤销的精确命中锚点（见 `select_entry`）。
#[must_use]
pub fn record_run(
    command: HistoryCommand,
    items: &[&ScanItem],
    report: &CleanReport,
) -> Option<String> {
    if report.success_count == 0 {
        return None;
    }
    let entry = HistoryEntry::from_report(command, items, report);
    let path = default_path();
    match record(&entry, &path) {
        Ok(()) => Some(entry.run_id),
        Err(e) => {
            log::warn!("写入清理账本失败（已忽略，不影响清理结果）: {e:?}");
            None
        }
    }
}

/// 读回全部账本记录（按文件顺序，即时间先后）。
///
/// 单行解析失败**跳过并 warn**，不因一条坏行丢掉整本账本（前向兼容/半写入容错）。
/// 文件不存在返回空 Vec（首次使用的正常态，非错误）。
#[must_use]
pub fn load(path: &Path) -> Vec<HistoryEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("读取清理账本失败 {path:?}: {e:?}");
            }
            return Vec::new();
        }
    };
    let mut entries = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<HistoryEntry>(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => log::warn!("跳过无法解析的账本行: {e}"),
        }
    }
    entries
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 生成 run-id：纳秒时间戳（十六进制）+ 进程号。纯本地唯一，够防人机节奏下的碰撞。
fn gen_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{nanos:x}-{:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CleanedItem, SafetyLevel};
    use tempfile::tempdir;

    fn item(path: &str, size: u64, category: &str) -> ScanItem {
        ScanItem::new(PathBuf::from(path), size, SafetyLevel::Safe, category.to_string())
    }

    #[test]
    fn entry_roundtrips_through_json() {
        let entry = HistoryEntry {
            run_id: "abc-1".into(),
            timestamp: 1_700_000_000,
            command: HistoryCommand::Purge,
            freed: 1234,
            count: 2,
            categories: vec![CategoryStat { name: "node_modules".into(), size: 1234, count: 2 }],
            deleted_paths: vec![PathBuf::from("/a"), PathBuf::from("/b")],
            restorable: vec![RestoreEntry {
                original: PathBuf::from("/a"),
                trashed_to: PathBuf::from("/Users/x/.Trash/a"),
                trashed_ino: 4242,
            }],
        };
        let line = serde_json::to_string(&entry).unwrap();
        // 命令类型序列化为小写字符串，便于外部消费。
        assert!(line.contains("\"command\":\"purge\""), "command 应为小写字符串: {line}");
        let back: HistoryEntry = serde_json::from_str(&line).unwrap();
        assert_eq!(entry, back, "序列化-反序列化应完全往返");
    }

    #[test]
    fn record_appends_one_line_per_call() {
        let dir = tempdir().unwrap();
        // 用嵌套子目录验证 record 会自动建父目录。
        let path = dir.path().join("state/mc/history.jsonl");

        let e1 = HistoryEntry {
            run_id: "r1".into(),
            timestamp: 1,
            command: HistoryCommand::Clean,
            freed: 10,
            count: 1,
            categories: vec![],
            deleted_paths: vec![PathBuf::from("/x")],
            restorable: vec![],
        };
        let mut e2 = e1.clone();
        e2.run_id = "r2".into();
        e2.command = HistoryCommand::Purge;

        record(&e1, &path).unwrap();
        record(&e2, &path).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2, "两次 record 应得两行（append 非覆盖）");
        assert_eq!(loaded[0].run_id, "r1", "顺序应保持写入先后");
        assert_eq!(loaded[1].command, HistoryCommand::Purge);
    }

    #[test]
    fn load_skips_malformed_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let good = serde_json::to_string(&HistoryEntry {
            run_id: "ok".into(),
            timestamp: 5,
            command: HistoryCommand::Clean,
            freed: 1,
            count: 1,
            categories: vec![],
            deleted_paths: vec![],
            restorable: vec![],
        })
        .unwrap();
        // 中间夹一行坏 JSON + 一行空行，均应被跳过而非丢整本账本。
        std::fs::write(&path, format!("{good}\n这不是JSON\n\n{good}\n")).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2, "坏行/空行应跳过，保留 2 条有效记录");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let loaded = load(Path::new("/nonexistent/mc/history.jsonl"));
        assert!(loaded.is_empty(), "文件不存在应返回空 Vec（首次使用的正常态）");
    }

    #[test]
    fn from_report_aggregates_only_successful_by_category() {
        // 三项：两成功（分属两分类）、一失败。账本只应计成功项，且按分类聚合。
        let a = item("/cache/a", 100, "系统缓存");
        let b = item("/cache/b", 50, "系统缓存");
        let c = item("/logs/c", 999, "日志");
        let items: Vec<&ScanItem> = vec![&a, &b, &c];

        let mut report = CleanReport::default();
        report.add(CleanedItem { path: a.path.clone(), size: 100, success: true, error: None, trashed_to: None, trashed_ino: None });
        report.add(CleanedItem { path: b.path.clone(), size: 50, success: true, error: None, trashed_to: None, trashed_ino: None });
        report.add(CleanedItem {
            path: c.path.clone(),
            size: 999,
            success: false,
            error: Some("权限不足".into()),
            trashed_to: None,
            trashed_ino: None,
        });

        let entry = HistoryEntry::from_report(HistoryCommand::Clean, &items, &report);

        assert_eq!(entry.command, HistoryCommand::Clean);
        assert_eq!(entry.freed, 150, "只计两个成功项的 size");
        assert_eq!(entry.count, 2);
        assert_eq!(entry.deleted_paths.len(), 2, "失败项不进 deleted_paths");
        assert!(!entry.deleted_paths.contains(&c.path), "失败的日志项不应出现");
        // 只有一个分类（系统缓存）有成功项；失败的"日志"分类不应出现。
        assert_eq!(entry.categories.len(), 1);
        assert_eq!(entry.categories[0].name, "系统缓存");
        assert_eq!(entry.categories[0].size, 150);
        assert_eq!(entry.categories[0].count, 2);
    }

    #[test]
    fn default_path_points_to_local_state() {
        let p = default_path();
        assert!(
            p.ends_with(".local/state/mc/history.jsonl"),
            "默认路径应落在 ~/.local/state/mc/history.jsonl，实际: {p:?}"
        );
    }

    #[test]
    fn from_report_builds_restorable_only_from_successful_captured_items() {
        // 四项：a 成功且路径+inode 齐全、b 成功但无落点、d 成功但有路径无 inode（不成对）、c 失败。
        // restorable 只应含 a（路径与 inode 必须成对）。
        let a = item("/cache/a", 100, "系统缓存");
        let b = item("/cache/b", 50, "系统缓存");
        let d = item("/cache/d", 20, "系统缓存");
        let c = item("/logs/c", 999, "日志");
        let items: Vec<&ScanItem> = vec![&a, &b, &d, &c];

        let mut report = CleanReport::default();
        report.add(CleanedItem {
            path: a.path.clone(),
            size: 100,
            success: true,
            error: None,
            trashed_to: Some(PathBuf::from("/Users/x/.Trash/a")),
            trashed_ino: Some(4242),
        });
        report.add(CleanedItem {
            path: b.path.clone(),
            size: 50,
            success: true,
            error: None,
            trashed_to: None,
            trashed_ino: None,
        });
        report.add(CleanedItem {
            path: d.path.clone(),
            size: 20,
            success: true,
            error: None,
            trashed_to: Some(PathBuf::from("/Users/x/.Trash/d")),
            trashed_ino: None, // 路径有但 inode 缺 → 不成对，不计入
        });
        report.add(CleanedItem {
            path: c.path.clone(),
            size: 999,
            success: false,
            error: Some("权限不足".into()),
            trashed_to: Some(PathBuf::from("/Users/x/.Trash/c")),
            trashed_ino: Some(9), // 失败项即便有落点也不应计入
        });

        let entry = HistoryEntry::from_report(HistoryCommand::Clean, &items, &report);

        assert_eq!(entry.restorable.len(), 1, "只含成功且路径+inode 成对的项");
        assert_eq!(entry.restorable[0].original, a.path);
        assert_eq!(entry.restorable[0].trashed_to, PathBuf::from("/Users/x/.Trash/a"));
        assert_eq!(entry.restorable[0].trashed_ino, 4242);
        // deleted_paths 仍按原语义含全部成功项（a、b、d），与 restorable 解耦。
        assert_eq!(entry.deleted_paths.len(), 3);
    }

    #[test]
    fn load_old_entry_without_restorable_field_defaults_empty() {
        // 保护 #24 已写入的历史账本：不含 restorable 字段的旧行必须能加载，restorable 为空。
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let old_line = r#"{"run_id":"old-1","timestamp":100,"command":"clean","freed":42,"count":1,"categories":[],"deleted_paths":["/old/x"]}"#;
        std::fs::write(&path, format!("{old_line}\n")).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 1, "旧行应正常加载");
        assert_eq!(loaded[0].run_id, "old-1");
        assert_eq!(loaded[0].freed, 42);
        assert!(loaded[0].restorable.is_empty(), "缺失 restorable 字段应默认空 Vec");
        assert_eq!(loaded[0].deleted_paths, vec![PathBuf::from("/old/x")]);
    }

    // --- select_entry：CLI mc undo 与 GUI 撤销共享的选取真源（从 cli/undo.rs 上提）---

    fn sel_entry(run_id: &str, restorable: Vec<&str>) -> HistoryEntry {
        HistoryEntry {
            run_id: run_id.into(),
            timestamp: 1,
            command: HistoryCommand::Clean,
            freed: 0,
            count: restorable.len(),
            categories: vec![],
            deleted_paths: vec![],
            restorable: restorable
                .into_iter()
                .map(|p| RestoreEntry {
                    original: PathBuf::from(p),
                    trashed_to: PathBuf::from(format!("/T/{p}")),
                    trashed_ino: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn select_none_picks_latest_with_mapping() {
        // 最后一条有映射 → 选它。
        let entries = vec![sel_entry("r1", vec!["/a"]), sel_entry("r2", vec!["/b"])];
        assert_eq!(select_entry(&entries, None).unwrap().run_id, "r2");
    }

    #[test]
    fn select_none_skips_trailing_entries_without_mapping() {
        // 最后一条无映射（旧记录），更早一条有 → 选更早那条有映射的。
        let entries = vec![sel_entry("r1", vec!["/a"]), sel_entry("r2", vec![])];
        assert_eq!(select_entry(&entries, None).unwrap().run_id, "r1");
    }

    #[test]
    fn select_none_returns_none_when_no_mapping_anywhere() {
        let entries = vec![sel_entry("r1", vec![]), sel_entry("r2", vec![])];
        assert!(select_entry(&entries, None).is_none());
    }

    #[test]
    fn select_by_run_id_hits_exact() {
        let entries = vec![sel_entry("r1", vec!["/a"]), sel_entry("r2", vec!["/b"])];
        assert_eq!(select_entry(&entries, Some("r1")).unwrap().run_id, "r1");
    }

    #[test]
    fn select_by_run_id_missing_returns_none() {
        let entries = vec![sel_entry("r1", vec!["/a"])];
        assert!(select_entry(&entries, Some("nope")).is_none());
    }

    #[test]
    fn select_empty_ledger_returns_none() {
        let entries: Vec<HistoryEntry> = vec![];
        assert!(select_entry(&entries, None).is_none());
        assert!(select_entry(&entries, Some("r1")).is_none());
    }

    #[test]
    fn select_by_run_id_ignores_newer_unrelated_entry() {
        // 共享账本竞态：给定旧 run_id，即便存在更新的、不同 run_id 的含落点条目，也只命中旧条目。
        let entries = vec![sel_entry("old", vec!["/a"]), sel_entry("newer", vec!["/b"])];
        let hit = select_entry(&entries, Some("old")).unwrap();
        assert_eq!(hit.run_id, "old");
        assert_eq!(hit.restorable[0].original, PathBuf::from("/a"));
    }
}
