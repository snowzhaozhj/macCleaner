//! 命令启动：菜单键分发 + `start_command`（Clean/Purge/Analyze/Uninstall 后台线程装配）。
//!
//! 每条命令 spawn 独立后台线程，用 `catch_unwind` 包裹并经 `TuiReporter` 回发事件；
//! Analyze 走独立 `AnalyzeEvent` channel + `IncrementalTreeBuilder` 增量建树。

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Receiver;
use crossterm::event::KeyCode;
use mc_core::engine::Engine;
use mc_core::models::DirNode;
use mc_core::platform;
use mc_core::progress::{AnalyzeEvent, ProgressEvent, ProgressReporter};

use crate::app::{ActiveCommand, App, AppState};
use crate::event::EventHandler;
use crate::reporter::TuiReporter;
use mc_core::IncrementalTreeBuilder;

/// 菜单页键盘处理
pub(crate) fn handle_menu_key(
    app: &mut App,
    key: KeyCode,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    match key {
        KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Up | KeyCode::Char('k')
            if app.menu_index > 0 => {
                app.menu_index -= 1;
            }
        KeyCode::Down | KeyCode::Char('j')
            if app.menu_index < 3 => {
                app.menu_index += 1;
            }
        KeyCode::Enter => {
            let cmd = match app.menu_index {
                0 => ActiveCommand::Clean,
                1 => ActiveCommand::Uninstall,
                2 => ActiveCommand::Analyze,
                3 => ActiveCommand::Purge,
                _ => return,
            };
            app.active_command = Some(cmd);
            start_command(app, cmd, events, analyze_rx, tree_builder);
        }
        _ => {}
    }
}

/// 启动命令执行
fn start_command(
    app: &mut App,
    cmd: ActiveCommand,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    // 每次命令从干净状态开始：清掉上一次可能残留的结果/标记/展开态/光标，
    // 确保新命令不会看到上一次命令的检测结果（与 reporter 丢弃取消事件形成双保险）。
    app.scan_result = None;
    app.expanded.clear();
    app.marked.clear();
    app.result_cursor = 0;
    // 排空进度队列：丢弃上一次扫描（可能刚被取消）残留在 channel 中、尚未消费的事件，
    // 否则它们会在本次扫描的 Scanning 态被消费、串入新命令的列表。
    while events.progress_rx.try_recv().is_ok() {}

    match cmd {
        ActiveCommand::Clean => {
            app.cancel_flag = Arc::new(AtomicBool::new(false));
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tx = events.progress_sender();
            let cancel = app.cancel_flag.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match Engine::scan_clean(&reporter) {
                        Ok(_result) => {}
                        Err(e) => {
                            reporter.on_event(ProgressEvent::Error(e.to_string()));
                        }
                    }
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {e:?}")));
                }
            });
        }
        ActiveCommand::Purge => {
            app.cancel_flag = Arc::new(AtomicBool::new(false));
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tx = events.progress_sender();
            let path = app.purge_path.clone();
            let cancel = app.cancel_flag.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match Engine::scan_purge(&path, &reporter) {
                        Ok(_result) => {}
                        Err(e) => {
                            reporter.on_event(ProgressEvent::Error(e.to_string()));
                        }
                    }
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {e:?}")));
                }
            });
        }
        ActiveCommand::Analyze => {
            // 通过独立 AnalyzeEvent channel + IncrementalTreeBuilder 实现增量构建
            let (tx, rx) = crossbeam_channel::bounded::<AnalyzeEvent>(4096);
            *analyze_rx = Some(rx);
            *tree_builder = Some(IncrementalTreeBuilder::new());

            let home = platform::get_home_dir();
            let root_name = home
                .file_name().map_or_else(|| "~".into(), |f| f.to_string_lossy().to_string());
            app.state = AppState::AnalyzingLive {
                tree_root: DirNode::new_dir(home.clone(), root_name),
                nav_path: Vec::new(),
                cursor: 0,
                cursor_stack: Vec::new(),
                file_count: 0,
                total_size: 0,
                user_navigated: false,
            };

            thread::spawn(move || {
                // 用 catch_unwind 包裹遍历，确保 Finished 始终被发送
                let tx_clone = tx.clone();
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        // 取消信号：Receiver drop（用户离开 analyze）→ send 失败 → 置 stop。
                        // jwalk 按 entry、park 按批查此 flag 及时中止遍历。
                        let stop = std::sync::atomic::AtomicBool::new(false);
                        let mut count = 0u64;
                        let mut total = 0u64;
                        mc_core::analyze_walk(
                            &home,
                            || stop.load(std::sync::atomic::Ordering::Relaxed),
                            |name, path, size, is_file| {
                                if tx_clone
                                    .send(AnalyzeEvent::Entry { name, path, size, is_file })
                                    .is_err()
                                {
                                    stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                    return;
                                }
                                if is_file {
                                    count += 1;
                                    total += size;
                                    if count.is_multiple_of(500) {
                                        let _ = tx_clone.send(AnalyzeEvent::Progress {
                                            file_count: count,
                                            total_size: total,
                                        });
                                    }
                                }
                            },
                        );
                    }));
                // 无论正常完成还是 panic，都发送 Finished
                let _ = tx.send(AnalyzeEvent::Finished);
                if let Err(e) = result {
                    eprintln!("Analyze 遍历线程 panic: {e:?}");
                }
            });
        }
        ActiveCommand::Uninstall => {
            // Uninstall 与 Clean/Purge 同款后台流式：list_apps 的 calc_app_size 重活
            // 不再同步阻塞主线程（曾致按 Enter 后菜单冻结），而是边扫边 Found 追加。
            app.cancel_flag = Arc::new(AtomicBool::new(false));
            app.state = AppState::Scanning {
                progress_text: "扫描应用中...".into(),
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tx = events.progress_sender();
            let cancel = app.cancel_flag.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Engine::scan_uninstall(&reporter);
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {e:?}")));
                }
            });
        }
    }
}
