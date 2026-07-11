//! 删除执行子系统：确认接受 → 后台废纸篓线程 → 启动清理（Results / 分析器两条发起路径）。
//!
//! `CONFIRM_TOKEN`（type-to-confirm 令牌）与删除线程收敛于此；live 删除先收尾建树再删，
//! 经 `crate::progress::transition_to_sorting` 过渡到 Sorting。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Receiver;
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, DirNode, SafetyLevel};
use mc_core::progress::{AnalyzeEvent, ProgressEvent, ProgressReporter};
use mc_core::rules::deletion_evidence_for_paths;

use crate::app::{self, App, AppState, ConfirmItem, PendingAnalyzerDelete};
use crate::event::EventHandler;
use crate::reporter::TuiReporter;
use mc_core::IncrementalTreeBuilder;
use crate::progress::transition_to_sorting;

/// 后台删除线程：把 (路径, 大小) 清单移入废纸篓，完成后 send `CleaningDone`。
/// 含 Risky 项时需输入的确认 token（type-to-confirm，D4）。确认框提示文案复用此常量避免漂移。
pub const CONFIRM_TOKEN: &str = "delete";

/// 对 Analyze 待删路径重新取核心层证据。批量入口只解析一次内置规则，且未知路径统一为 Risky。
pub(crate) fn analyzer_confirm_items(items: &[(PathBuf, u64)]) -> Vec<ConfirmItem> {
    let paths: Vec<PathBuf> = items.iter().map(|(path, _)| path.clone()).collect();
    items
        .iter()
        .zip(deletion_evidence_for_paths(&paths))
        .map(|((path, size), (safety, impact, recovery))| ConfirmItem {
            path: path.clone(),
            size: *size,
            safety,
            category: String::new(),
            impact,
            recovery,
        })
        .collect()
}

/// Analyze 从展示确认到真正执行之间仍可能发生 root marker 变化。执行前重新分类；若某项
/// 从 Safe/Moderate 升级为 Risky 且未在本轮确认框以 Risky 展示，就重新打开强确认。
fn analyzer_delete_authorized(
    app: &mut App,
    items: &[(PathBuf, u64)],
    confirmed_risky_paths: &HashSet<PathBuf>,
) -> bool {
    let refreshed = analyzer_confirm_items(items);
    enforce_analyzer_confirmation(app, refreshed, confirmed_risky_paths)
}

fn enforce_analyzer_confirmation(
    app: &mut App,
    refreshed: Vec<ConfirmItem>,
    confirmed_risky_paths: &HashSet<PathBuf>,
) -> bool {
    let has_new_risky = refreshed.iter().any(|item| {
        item.safety == SafetyLevel::Risky && !confirmed_risky_paths.contains(&item.path)
    });
    if has_new_risky {
        app.confirm_delete = Some(refreshed);
        app.confirm_input.clear();
        app.confirm_scroll = 0;
        app.status_message = Some("路径安全状态已变化，请重新确认危险项".into());
        return false;
    }
    true
}

/// 执行已确认的删除：把确认项映射回 (path, size) 交给删除线程（KTD8：线程签名不变）。
pub(crate) fn confirm_accept(
    app: &mut App,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    let risky_confirmed = app.confirm_input.eq_ignore_ascii_case(CONFIRM_TOKEN);
    app.confirm_input.clear();
    app.confirm_scroll = 0;
    if let Some(list) = app.confirm_delete.take() {
        // 防御性后端闸：即使未来调用方绕过按键层直接调用本函数，Risky 也不能无 token 执行。
        if list.iter().any(|item| item.safety == SafetyLevel::Risky) && !risky_confirmed {
            app.confirm_delete = Some(list);
            return;
        }
        let confirmed_risky_paths: HashSet<PathBuf> = if risky_confirmed {
            list.iter()
                .filter(|item| item.safety == SafetyLevel::Risky)
                .map(|item| item.path.clone())
                .collect()
        } else {
            HashSet::new()
        };
        let items: Vec<(PathBuf, u64)> = list.iter().map(|i| (i.path.clone(), i.size)).collect();
        // live/Sorting 在后台 finalize 前先复核一次；稳定 Analyzing 没有异步边界，
        // 由 start_cleaning_from_analyzer 在真正启动删除线程前做唯一一次最终复核。
        let before_finalize = matches!(
            app.state,
            AppState::AnalyzingLive { .. } | AppState::Sorting
        );
        if before_finalize
            && !analyzer_delete_authorized(app, &items, &confirmed_risky_paths)
        {
            return;
        }
        // 分析器发起的删除：删后原地留在树内（暂存树剪枝恢复）；其余（Results）：删后走 Done → 菜单。
        if matches!(app.state, AppState::Analyzing { .. }) {
            app.clean_request = Vec::new();
            start_cleaning_from_analyzer(app, items, &confirmed_risky_paths, events);
        } else if matches!(app.state, AppState::AnalyzingLive { .. }) {
            // live 态删除（KTD1）：先收尾——停遍历 + finalize 部分树 → Sorting → Analyzing；
            // 暂存待删清单，待 SortDone 在稳定树上执行（见 run_app 的 SortDone 分支）。
            app.clean_request = Vec::new();
            app.pending_analyzer_delete = Some(PendingAnalyzerDelete {
                items,
                confirmed_risky_paths,
            });
            transition_to_sorting(app, analyze_rx, tree_builder, sort_rx);
        } else if matches!(app.state, AppState::Sorting) {
            // 竞态：确认框展示期间扫描自然完成已进入 Sorting（finalize 进行中）。此确认必来自
            // live 态（Sorting 仅由 AnalyzingLive 进入，且 live 删除的 transition 已消费过
            // confirm_delete），故仍属 live 删除——暂存待删让 SortDone 统一在稳定树上执行，
            // 不落入 Results 删除路径（否则违背 R3 且丢弃已排序树，审查条目 #1）。
            app.clean_request = Vec::new();
            app.pending_analyzer_delete = Some(PendingAnalyzerDelete {
                items,
                confirmed_risky_paths,
            });
        } else {
            // Scanning 态删除：先收尾扫描——置 cancel_flag 停剩余规则，避免边删边扫的
            // Found 事件混入 Cleaning 态（KTD1）。残留 Found 由 handle_progress 的
            // 非 Scanning 守卫忽略，无需额外排空。
            if matches!(app.state, AppState::Scanning { .. }) {
                app.cancel_flag.store(true, Ordering::Relaxed);
            }
            // 暂存完整待删清单，供 Done 屏计算成功/失败明细与分类小结（KTD6）。
            app.clean_request = list;
            start_cleaning(app, items, events);
        }
    }
}

fn spawn_trash_thread(items: Vec<(PathBuf, u64)>, events: &EventHandler) {
    let tx = events.progress_sender();
    let cancel = Arc::new(AtomicBool::new(false));
    thread::spawn(move || {
        let reporter = TuiReporter::new(tx, cancel);
        use mc_core::models::ScanItem;
        let scan_items: Vec<ScanItem> = items
            .iter()
            .map(|(path, size)| ScanItem::new(path.clone(), *size, SafetyLevel::Safe, String::new()))
            .collect();
        let refs: Vec<&ScanItem> = scan_items.iter().collect();

        // 与四条扫描线程一致，用 catch_unwind 包裹删除，确保 panic 也回发 Error。
        // 否则 Cleaner::execute 恒返回 Ok 使 panic 成为唯一失败出口，一旦 panic，
        // 主线程收不到 CleaningDone/Error，而 Cleaning 态屏蔽除 Ctrl+C 外全部按键 → 卡死
        // （经分析器删除路径触发时还会连带丢失暂存的整棵树）。Error 分支在 Cleaning 态会转 Done 解卡。
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Engine::clean(&refs, DeleteMode::Trash, &reporter)
        }));
        match result {
            Ok(Ok(_report)) => {}
            Ok(Err(e)) => {
                reporter.on_event(ProgressEvent::Error(e.to_string()));
            }
            Err(_) => {
                reporter.on_event(ProgressEvent::Error("删除线程内部错误（panic）".into()));
            }
        }
    });
}

/// 启动清理（Results 页发起）：转入 Cleaning，完成后走 Done → 菜单。
fn start_cleaning(app: &mut App, items: Vec<(PathBuf, u64)>, events: &EventHandler) {
    if items.is_empty() {
        return;
    }
    app.marked.clear();
    app.state = AppState::Cleaning {
        progress_text: "准备清理...".into(),
    };
    spawn_trash_thread(items, events);
}

/// 启动清理（磁盘分析器发起）：先把当前树与导航暂存到 `analyzer_return`（含待删路径），
/// 再转入 Cleaning 后台删除。完成后由 `CleaningDone` 分支剪除已删节点并**原地返回分析器**，
/// 而非像 Results 那样走 Done → 菜单（修复"删除后莫名退出、整棵树丢失"）。
pub(crate) fn start_cleaning_from_analyzer(
    app: &mut App,
    items: Vec<(PathBuf, u64)>,
    confirmed_risky_paths: &HashSet<PathBuf>,
    events: &EventHandler,
) {
    if items.is_empty() {
        return;
    }
    // live Analyze 需先后台 finalize；这段可能跨越数秒，因此在真正启动删除线程前再复核一次。
    if !analyzer_delete_authorized(app, &items, confirmed_risky_paths) {
        return;
    }
    let placeholder = AppState::Cleaning {
        progress_text: "准备清理...".into(),
    };
    if let AppState::Analyzing {
        tree_root,
        nav_path,
        cursor,
        cursor_stack,
    } = std::mem::replace(&mut app.state, placeholder)
    {
        app.analyzer_return = Some(app::AnalyzerReturn {
            tree: tree_root,
            nav_path,
            cursor,
            cursor_stack,
            deleted: items.iter().map(|(p, _)| p.clone()).collect(),
        });
    }
    app.marked.clear();
    spawn_trash_thread(items, events);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn confirm_item(safety: SafetyLevel) -> ConfirmItem {
        ConfirmItem {
            path: PathBuf::from("/unknown/user-data"),
            size: 42,
            safety,
            category: String::new(),
            impact: "可能丢失数据".into(),
            recovery: "从废纸篓恢复".into(),
        }
    }

    #[test]
    fn analyzer_risk_upgrade_reopens_strong_confirmation() {
        let mut app = App::new();
        assert!(enforce_analyzer_confirmation(
            &mut app,
            vec![confirm_item(SafetyLevel::Moderate)],
            &HashSet::new(),
        ));
        assert!(app.confirm_delete.is_none());

        assert!(!enforce_analyzer_confirmation(
            &mut app,
            vec![confirm_item(SafetyLevel::Risky)],
            &HashSet::new(),
        ));
        assert!(app.confirm_has_risky(), "升级为 Risky 后必须重新打开强确认");
        assert!(app.status_message.as_deref().is_some_and(|s| s.contains("重新确认")));
    }

    #[test]
    fn analyzer_risky_recheck_accepts_existing_path_authorization() {
        let mut app = App::new();
        let confirmed = HashSet::from([PathBuf::from("/unknown/user-data")]);
        assert!(enforce_analyzer_confirmation(
            &mut app,
            vec![confirm_item(SafetyLevel::Risky)],
            &confirmed,
        ));
        assert!(app.confirm_delete.is_none());
    }

    #[test]
    fn analyzer_risky_authorization_does_not_cover_new_risky_path() {
        let mut app = App::new();
        let first_path = PathBuf::from("/unknown/first");
        let second_path = PathBuf::from("/unknown/second");
        let confirmed = HashSet::from([first_path.clone()]);
        let mut first = confirm_item(SafetyLevel::Risky);
        first.path = first_path;
        let mut second = confirm_item(SafetyLevel::Risky);
        second.path = second_path.clone();

        assert!(!enforce_analyzer_confirmation(
            &mut app,
            vec![first, second],
            &confirmed,
        ));
        assert!(
            app.confirm_delete
                .as_ref()
                .is_some_and(|items| items.iter().any(|item| item.path == second_path)),
            "已有 Risky 项的口令不能静默授权后来升级的另一条路径"
        );
    }

    #[test]
    fn confirm_accept_cannot_bypass_risky_token() {
        let mut app = App::new();
        app.confirm_delete = Some(vec![confirm_item(SafetyLevel::Risky)]);
        let events = EventHandler::new();

        confirm_accept(&mut app, &events, &mut None, &mut None, &mut None);

        assert!(app.confirm_has_risky(), "直接调用 confirm_accept 也不能绕过 token");
        assert!(matches!(app.state, AppState::Menu), "未授权时不得进入 Cleaning");
    }
}
