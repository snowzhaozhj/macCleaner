//! Analyze 命令：流式增量建磁盘占用树、按标记路径移废纸篓删除。
//! 复用 `mc_core::analyze_walk` + 上提的 `IncrementalTreeBuilder`，不另建第二套建树（R1）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use mc_core::engine::Engine;
use mc_core::models::{CleanReport, DeleteMode, DirNode, SafetyLevel, ScanItem};
use mc_core::progress::{AnalyzeEvent, ProgressEvent};
use mc_core::{analyze_walk, IncrementalTreeBuilder};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};

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

/// 从树 + 标记集构造待删 `ScanItem`（analyze 无规则来源，安全等级取 `Safe`、空分类——
/// 与 TUI 一致；Risky 门槛在 UI 层的 type-to-confirm，删除去向恒为废纸篓）。纯函数便于单测。
pub fn marked_items<H: std::hash::BuildHasher>(
    tree: &DirNode,
    marked: &HashSet<PathBuf, H>,
) -> Vec<ScanItem> {
    let mut pairs = Vec::new();
    collect_marked(tree, marked, &mut pairs);
    pairs
        .into_iter()
        .map(|(path, size)| ScanItem::new(path, size, SafetyLevel::Safe, String::new()))
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
#[tauri::command]
pub async fn delete_marked(
    app: AppHandle,
    paths: Vec<PathBuf>,
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
        assert_eq!(items[0].safety, SafetyLevel::Safe, "analyze 项取 Safe");
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
