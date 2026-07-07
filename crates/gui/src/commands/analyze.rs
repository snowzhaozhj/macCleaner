//! Analyze 命令：流式增量建磁盘占用树、按标记路径移废纸篓删除。
//! 复用 `mc_core::analyze_walk` + 上提的 `IncrementalTreeBuilder`，不另建第二套建树（R1）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use mc_core::engine::Engine;
use mc_core::models::{CleanReport, DeleteMode, DirNode, SafetyLevel, ScanItem};
use mc_core::progress::{AnalyzeEvent, ProgressEvent};
use mc_core::rules::evidence_for_path;
use mc_core::{analyze_walk, IncrementalTreeBuilder};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

use crate::commands::is_confirmed;
use crate::reporter::TauriReporter;
use crate::AppState;

/// 每处理 N 个文件向前端推一次进度快照（对齐核心 500 的粒度）。
const PROGRESS_EVERY: u64 = 500;

fn dir_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// 递归收集被标记节点的 (path, size)。祖先命中即收集，不再深入其子（整目录删）。
fn collect_marked<H: std::hash::BuildHasher>(
    node: &DirNode,
    marked: &HashSet<PathBuf, H>,
    out: &mut Vec<(PathBuf, u64)>,
) {
    if marked.contains(&node.path) {
        out.push((node.path.clone(), node.size));
        return;
    }
    for child in &node.children {
        collect_marked(child, marked, out);
    }
}

/// 分析器项来自 `DirNode`、无规则元数据：按路径回查规则证据（`evidence_for_path`）推断安全等级，
/// 使 Risky 路径（Docker 卷/Xcode Archives/AVD 等）经分析器删除时也带 Risky、能触发 type-to-confirm；
/// 未命中任何规则的普通路径默认 `Safe`（对齐 TUI `analyzer_ops` 的 KTD8 语义，R-review codex-P1）。
fn safety_for(path: &Path) -> SafetyLevel {
    evidence_for_path(path).map_or(SafetyLevel::Safe, |(safety, ..)| safety)
}

/// 从树 + 标记集构造待删 `ScanItem`。安全等级按路径回查规则（见 `safety_for`），删除去向恒废纸篓。
/// 纯函数便于单测。
pub fn marked_items<H: std::hash::BuildHasher>(
    tree: &DirNode,
    marked: &HashSet<PathBuf, H>,
) -> Vec<ScanItem> {
    let mut pairs = Vec::new();
    collect_marked(tree, marked, &mut pairs);
    pairs
        .into_iter()
        .map(|(path, size)| {
            let safety = safety_for(&path);
            ScanItem::new(path, size, safety, String::new())
        })
        .collect()
}

/// 一条标记路径的安全分级（供前端在打开确认弹窗前渲染三通道 + 决定是否要 type-to-confirm）。
#[derive(Debug, Serialize)]
pub struct PathSafety {
    pub path: PathBuf,
    pub safety: SafetyLevel,
}

/// 为前端标记的路径集回查安全等级（不触碰磁盘、无删除）。前端据此在 `ConfirmDelete`
/// 显示 Risky 三通道并对含 Risky 的删除要求输入确认口令（R-review codex-P1）。
#[tauri::command]
pub async fn classify_marked(paths: Vec<PathBuf>) -> Vec<PathSafety> {
    paths
        .into_iter()
        .map(|path| {
            let safety = safety_for(&path);
            PathSafety { path, safety }
        })
        .collect()
}

/// 流式增量建树。进度经 `on_event` 推前端；finalize 后的树存入 `last_analyze`
/// 供 `delete_marked` 收集，同时回传前端导航。
#[tauri::command]
pub async fn analyze(
    app: AppHandle,
    root: PathBuf,
    on_event: Channel<AnalyzeEvent>,
) -> Result<DirNode, String> {
    // begin_operation 安装本次分析专属取消 flag（R-review：不再复位共享 flag）。
    let (cancelled, last_analyze) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_analyze.clone())
    };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let mut tree = DirNode::new_dir(root.clone(), dir_name(&root));
        let mut builder = IncrementalTreeBuilder::new();
        let mut file_count: u64 = 0;
        analyze_walk(
            &root,
            || cancelled.load(Ordering::Relaxed),
            |name, path, size, is_file| {
                builder.integrate_entry(&mut tree, name, path, size, is_file);
                if is_file {
                    file_count += 1;
                    if file_count.is_multiple_of(PROGRESS_EVERY) {
                        let _ = on_event.send(AnalyzeEvent::Progress {
                            file_count,
                            total_size: tree.size,
                        });
                    }
                }
            },
        );
        // 取消后 analyze_walk 提前返回，树是**不完整**的——此时不 finalize、不发 Finished、
        // 不存树，返回 None，避免前端把半截结果当完整分析继续删除（R-review codex-P1）。
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        IncrementalTreeBuilder::finalize(&mut tree);
        let _ = on_event.send(AnalyzeEvent::Finished);
        Some(tree)
    })
    .await
    .map_err(|e| format!("分析线程异常: {e}"))?;
    let tree = outcome.ok_or_else(|| "分析已取消".to_string())?;
    *last_analyze.lock().map_err(|_| "状态锁毒化".to_string())? = Some(tree.clone());
    Ok(tree)
}

/// 移废纸篓删除被标记的目录/文件（恒用 `DeleteMode::Trash`，R7/AE3）。
/// 待删项从上次分析树按标记路径收集，删除后前端据 `CleaningDone.deleted_paths` 原地剪树。
///
/// `confirm_token`：与 `clean` 同——若标记项含 Risky（经 `evidence_for_path` 回查，如 Docker 卷、
/// Xcode Archives），须携带有效确认口令方可删，防分析器绕过 type-to-confirm（R-review codex-P1）。
#[tauri::command]
pub async fn delete_marked(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanReport, String> {
    let (cancelled, last_analyze) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_analyze.clone())
    };
    let marked: HashSet<PathBuf> = paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 先在短临界区内收集 owned 待删项，随即 drop 锁——避免删除全程持锁，
        // 一旦 Engine::clean panic 会毒化 last_analyze，永久使后续 analyze/删除失败（R-review）。
        let items = {
            let guard = last_analyze.lock().map_err(|_| "状态锁毒化".to_string())?;
            let tree = guard.as_ref().ok_or_else(|| "无分析结果可删除".to_string())?;
            marked_items(tree, &marked)
        };
        // 后端闸：含 Risky 必须有有效确认口令（防分析器绕过前端 type-to-confirm）。
        if items.iter().any(|i| i.safety == SafetyLevel::Risky) && !is_confirmed(&confirm_token) {
            return Err("含危险项，需输入确认口令方可删除".to_string());
        }
        let refs: Vec<&ScanItem> = items.iter().collect();
        let reporter = TauriReporter::new(on_event, cancelled);
        Engine::clean(&refs, DeleteMode::Trash, &reporter).map_err(|e| format!("删除失败: {e}"))
    })
    .await
    .map_err(|e| format!("删除线程异常: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一棵 root/[a(dir)/f1(100), big(dir)/f2(500)], 手工组装（不走 walk）。
    fn sample_tree() -> DirNode {
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        let mut a = DirNode::new_dir(PathBuf::from("/r/a"), "a".into());
        a.children.push(DirNode::new_file(PathBuf::from("/r/a/f1"), "f1".into(), 100));
        a.size = 100;
        let mut big = DirNode::new_dir(PathBuf::from("/r/big"), "big".into());
        big.children.push(DirNode::new_file(PathBuf::from("/r/big/f2"), "f2".into(), 500));
        big.size = 500;
        root.size = 600;
        root.children.push(a);
        root.children.push(big);
        root
    }

    #[test]
    fn marked_dir_collected_whole_not_descended() {
        let tree = sample_tree();
        let marked: HashSet<PathBuf> = [PathBuf::from("/r/big")].into_iter().collect();
        let items = marked_items(&tree, &marked);
        assert_eq!(items.len(), 1, "标记目录整体收集，不下探其子");
        assert_eq!(items[0].path, PathBuf::from("/r/big"));
        assert_eq!(items[0].size, 500);
        // /r/big 不匹配任何规则 → evidence_for_path 返回 None → 默认 Safe。
        assert_eq!(items[0].safety, SafetyLevel::Safe, "未命中规则的普通路径默认 Safe");
    }

    #[test]
    fn marked_risky_path_classified_from_rules() {
        // 命中 Risky 规则的路径（如 Xcode Archives）经 evidence_for_path 回查应为 Risky，
        // 而非一律 Safe——这是分析器删除也能触发 type-to-confirm 的前提（R-review codex-P1）。
        let archives =
            mc_core::platform::get_home_dir().join("Library/Developer/Xcode/Archives");
        let mut root = DirNode::new_dir(archives.clone(), "Archives".into());
        root.size = 1000;
        let marked: HashSet<PathBuf> = [archives.clone()].into_iter().collect();
        let items = marked_items(&root, &marked);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].safety, SafetyLevel::Risky, "Xcode Archives 应回查为 Risky");
    }

    #[test]
    fn unmarked_yields_nothing() {
        let tree = sample_tree();
        assert!(marked_items(&tree, &HashSet::new()).is_empty());
    }

    #[test]
    fn multiple_marks_across_subtrees() {
        let tree = sample_tree();
        let marked: HashSet<PathBuf> =
            [PathBuf::from("/r/a/f1"), PathBuf::from("/r/big")].into_iter().collect();
        let items = marked_items(&tree, &marked);
        let paths: HashSet<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
        assert_eq!(paths, marked);
    }
}
