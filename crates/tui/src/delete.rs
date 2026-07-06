//! 删除执行子系统：确认接受 → 后台废纸篓线程 → 启动清理（Results / 分析器两条发起路径）。
//!
//! `CONFIRM_TOKEN`（type-to-confirm 令牌）与删除线程收敛于此；live 删除先收尾建树再删，
//! 经 `crate::progress::transition_to_sorting` 过渡到 Sorting。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Receiver;
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, DirNode};
use mc_core::progress::{AnalyzeEvent, ProgressEvent, ProgressReporter};

use crate::app::{self, App, AppState};
use crate::event::EventHandler;
use crate::reporter::TuiReporter;
use crate::tree_builder::IncrementalTreeBuilder;
use crate::progress::transition_to_sorting;

/// 后台删除线程：把 (路径, 大小) 清单移入废纸篓，完成后 send `CleaningDone`。
/// 含 Risky 项时需输入的确认 token（type-to-confirm，D4）。确认框提示文案复用此常量避免漂移。
pub const CONFIRM_TOKEN: &str = "delete";

/// 执行已确认的删除：把确认项映射回 (path, size) 交给删除线程（KTD8：线程签名不变）。
pub(crate) fn confirm_accept(
    app: &mut App,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    app.confirm_input.clear();
    app.confirm_scroll = 0;
    if let Some(list) = app.confirm_delete.take() {
        let items: Vec<(PathBuf, u64)> = list.iter().map(|i| (i.path.clone(), i.size)).collect();
        // 分析器发起的删除：删后原地留在树内（暂存树剪枝恢复）；其余（Results）：删后走 Done → 菜单。
        if matches!(app.state, AppState::Analyzing { .. }) {
            app.clean_request = Vec::new();
            start_cleaning_from_analyzer(app, items, events);
        } else if matches!(app.state, AppState::AnalyzingLive { .. }) {
            // live 态删除（KTD1）：先收尾——停遍历 + finalize 部分树 → Sorting → Analyzing；
            // 暂存待删清单，待 SortDone 在稳定树上执行（见 run_app 的 SortDone 分支）。
            app.clean_request = Vec::new();
            app.pending_analyzer_delete = items;
            transition_to_sorting(app, analyze_rx, tree_builder, sort_rx);
        } else if matches!(app.state, AppState::Sorting) {
            // 竞态：确认框展示期间扫描自然完成已进入 Sorting（finalize 进行中）。此确认必来自
            // live 态（Sorting 仅由 AnalyzingLive 进入，且 live 删除的 transition 已消费过
            // confirm_delete），故仍属 live 删除——暂存待删让 SortDone 统一在稳定树上执行，
            // 不落入 Results 删除路径（否则违背 R3 且丢弃已排序树，审查条目 #1）。
            app.clean_request = Vec::new();
            app.pending_analyzer_delete = items;
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
        use mc_core::models::{SafetyLevel, ScanItem};
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
pub(crate) fn start_cleaning_from_analyzer(app: &mut App, items: Vec<(PathBuf, u64)>, events: &EventHandler) {
    if items.is_empty() {
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
