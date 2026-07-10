//! Analyze 命令：流式增量建磁盘占用树、按标记路径移废纸篓删除。
//! 复用 `mc_core::analyze_walk` + 上提的 `IncrementalTreeBuilder`，不另建第二套建树（R1）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use mc_core::engine::Engine;
use mc_core::models::{CleanReport, DeleteMode, DirNode, SafetyLevel, ScanItem};
use mc_core::progress::{AnalyzeEvent, ProgressEvent};
use mc_core::rules::deletion_evidence_for_paths;
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

/// 从树 + 标记集构造待删 `ScanItem`。安全等级与证据统一走核心删除分类；未命中规则的路径
/// 保守归为 Risky，不能把用户文档等任意路径当作 Safe 绕过 type-to-confirm。删除去向恒废纸篓。
/// 纯函数便于单测。
pub fn marked_items<H: std::hash::BuildHasher>(
    tree: &DirNode,
    marked: &HashSet<PathBuf, H>,
) -> Vec<ScanItem> {
    let mut pairs = Vec::new();
    collect_marked(tree, marked, &mut pairs);
    build_marked_items(pairs)
}

/// 为已从树中复制出的待删路径补齐安全证据。与树遍历拆开后，GUI 删除入口可先释放
/// `last_analyze` 锁，再执行规则解析和文件系统 marker 检查。
fn build_marked_items(pairs: Vec<(PathBuf, u64)>) -> Vec<ScanItem> {
    let paths: Vec<PathBuf> = pairs.iter().map(|(path, _)| path.clone()).collect();
    pairs
        .into_iter()
        .zip(deletion_evidence_for_paths(&paths))
        .map(|((path, size), (safety, impact, recovery))| {
            ScanItem::new(path, size, safety, String::new()).with_evidence(impact, recovery)
        })
        .collect()
}

/// 一条标记路径的删除分级与证据（供前端在打开确认弹窗前渲染三通道 + 决定是否要 type-to-confirm）。
#[derive(Debug, Serialize)]
pub struct PathSafety {
    pub path: PathBuf,
    pub safety: SafetyLevel,
    pub impact: String,
    pub recovery: String,
}

fn classify_paths(paths: Vec<PathBuf>) -> Vec<PathSafety> {
    let evidence = deletion_evidence_for_paths(&paths);
    paths
        .into_iter()
        .zip(evidence)
        .map(|(path, (safety, impact, recovery))| PathSafety {
            path,
            safety,
            impact,
            recovery,
        })
        .collect()
}

/// 为前端标记的路径集回查安全等级与证据（不执行删除；DirName 规则会读取路径元数据与
/// root marker）。前端据此在 `ConfirmDelete` 显示真实后果，并对含 Risky 的删除要求口令。
#[tauri::command]
pub async fn classify_marked(paths: Vec<PathBuf>) -> Vec<PathSafety> {
    classify_paths(paths)
}

/// 校验前端的强确认授权。口令只授权确认框中当时已经展示为 Risky 的路径；若另一项在
/// 展示后因 marker 变化升级为 Risky，必须拒绝本次请求，让用户重新查看证据并确认。
fn authorize_marked_delete(
    items: &[ScanItem],
    confirm_token: &str,
    confirmed_risky_paths: &HashSet<PathBuf>,
) -> Result<(), String> {
    let risky_paths: Vec<&PathBuf> = items
        .iter()
        .filter(|item| item.safety == SafetyLevel::Risky)
        .map(|item| &item.path)
        .collect();
    if risky_paths.is_empty() {
        return Ok(());
    }
    if !is_confirmed(confirm_token) {
        return Err("含危险项，需输入确认口令方可删除".to_string());
    }
    if risky_paths
        .into_iter()
        .any(|path| !confirmed_risky_paths.contains(path))
    {
        return Err("路径安全状态已变化，请重新确认危险项".to_string());
    }
    Ok(())
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
/// `confirm_token`：与 `clean` 同——若标记项含 Risky（包括未匹配任何规则的路径），须携带
/// 有效确认口令；`confirmed_risky_paths` 把口令绑定到确认框实际展示过的危险路径。
#[tauri::command]
pub async fn delete_marked(
    app: AppHandle,
    paths: Vec<PathBuf>,
    confirm_token: String,
    confirmed_risky_paths: Vec<PathBuf>,
    on_event: Channel<ProgressEvent>,
) -> Result<CleanReport, String> {
    let (cancelled, last_analyze) = {
        let state = app.state::<AppState>();
        (state.begin_operation(), state.last_analyze.clone())
    };
    let marked: HashSet<PathBuf> = paths.into_iter().collect();
    let confirmed_risky_paths: HashSet<PathBuf> = confirmed_risky_paths.into_iter().collect();
    tauri::async_runtime::spawn_blocking(move || {
        // 先在短临界区内只复制待删路径与大小，随即 drop 锁——规则解析和 marker
        // 文件系统检查也在锁外执行，避免慢磁盘无谓阻塞下一次 analyze/删除。
        // 一旦 Engine::clean panic 会毒化 last_analyze，永久使后续 analyze/删除失败（R-review）。
        let pairs = {
            let guard = last_analyze.lock().map_err(|_| "状态锁毒化".to_string())?;
            let tree = guard.as_ref().ok_or_else(|| "无分析结果可删除".to_string())?;
            let mut pairs = Vec::new();
            collect_marked(tree, &marked, &mut pairs);
            pairs
        };
        let items = build_marked_items(pairs);
        // 后端闸：未知路径也归 Risky；口令与确认时的 Risky 路径集合必须同时匹配。
        authorize_marked_delete(&items, &confirm_token, &confirmed_risky_paths)?;
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
        assert_eq!(items[0].safety, SafetyLevel::Risky, "未命中规则的路径必须保守归为 Risky");
        assert!(!items[0].impact.is_empty(), "未知路径也必须说明删除后果");
        assert!(!items[0].recovery.is_empty(), "未知路径也必须说明恢复方式");
    }

    #[test]
    fn marked_risky_path_classified_from_rules() {
        // 命中 Risky 规则的路径（如 Xcode Archives）经共享删除分类回查应为 Risky，
        // 而非一律 Safe——这是分析器删除也能触发 type-to-confirm 的前提（R-review codex-P1）。
        let archives =
            mc_core::platform::get_home_dir().join("Library/Developer/Xcode/Archives");
        let mut root = DirNode::new_dir(archives.clone(), "Archives".into());
        root.size = 1000;
        let marked: HashSet<PathBuf> = [archives.clone()].into_iter().collect();
        let items = marked_items(&root, &marked);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].safety, SafetyLevel::Risky, "Xcode Archives 应回查为 Risky");
        assert!(items[0].impact.contains("dSYM"), "应保留规则的真实后果证据");
        assert!(items[0].recovery.contains("不可恢复"), "应保留规则的恢复证据");
    }

    #[test]
    fn marked_safe_path_keeps_rule_evidence() {
        let cache = mc_core::platform::get_home_dir().join("Library/Caches");
        let mut root = DirNode::new_dir(cache.clone(), "Caches".into());
        root.size = 1000;
        let marked: HashSet<PathBuf> = [cache].into_iter().collect();
        let items = marked_items(&root, &marked);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].safety, SafetyLevel::Safe, "已知缓存路径仍应为 Safe");
        assert!(items[0].impact.contains("缓存"), "Safe 项也应把规则证据传给确认框");
        assert!(!items[0].recovery.is_empty(), "Safe 项应带恢复说明");
    }

    #[test]
    fn classify_unknown_path_returns_risky_with_evidence() {
        let classified = classify_paths(vec![PathBuf::from("/Users/tester/Documents/report.txt")])
            .pop()
            .expect("单路径分类应返回一项");
        assert_eq!(classified.safety, SafetyLevel::Risky);
        assert!(!classified.impact.is_empty());
        assert!(!classified.recovery.is_empty());
    }

    #[test]
    fn risky_delete_requires_token_and_matching_path_authorization() {
        let risky_path = PathBuf::from("/Users/tester/Documents/report.txt");
        let risky = ScanItem::new(
            risky_path.clone(),
            1,
            SafetyLevel::Risky,
            String::new(),
        );

        assert!(
            authorize_marked_delete(std::slice::from_ref(&risky), "", &HashSet::new()).is_err(),
            "Risky 路径无口令必须拒绝"
        );
        assert!(
            authorize_marked_delete(
                std::slice::from_ref(&risky),
                "delete",
                &HashSet::from([PathBuf::from("/Users/tester/Documents/other.txt")]),
            )
            .is_err(),
            "非空但不匹配的授权集合也不能放行"
        );
        assert!(authorize_marked_delete(
            std::slice::from_ref(&risky),
            "delete",
            &HashSet::from([risky_path.clone()]),
        )
        .is_ok());

        let second = ScanItem::new(
            PathBuf::from("/Users/tester/Documents/second.txt"),
            1,
            SafetyLevel::Risky,
            String::new(),
        );
        assert!(
            authorize_marked_delete(
                &[risky, second],
                "delete",
                &HashSet::from([risky_path]),
            )
            .is_err(),
            "多个 Risky 路径只授权其中一项时必须拒绝"
        );
    }

    #[test]
    fn safe_delete_does_not_require_token() {
        let safe = ScanItem::new(
            PathBuf::from("/Users/tester/Library/Caches/app"),
            1,
            SafetyLevel::Safe,
            String::new(),
        );
        assert!(authorize_marked_delete(&[safe], "", &HashSet::new()).is_ok());
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
