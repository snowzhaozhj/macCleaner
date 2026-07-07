//! 进度事件处理 + Analyze 编排：`handle_progress`（扫描/清理事件汇入 `App` 状态）、
//! 及 `AnalyzingLive` → `Sorting` → `Analyzing` 的编排（entry/finished/cancel/leave）。
//!
//! `CleaningDone` 分析器分支委托 `crate::analyzer_ops::restore_analyzer_after_delete`；
//! `transition_to_sorting` / `cancel_analyze_to_menu` 供 `delete` / `analyzer_ops` 调用。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crossbeam_channel::Receiver;
use mc_core::models::DirNode;
use mc_core::platform;
use mc_core::progress::{AnalyzeEvent, ProgressEvent};

use crate::analyzer_ops::restore_analyzer_after_delete;
use crate::app::{self, ActiveCommand, App, AppState};
use crate::tree_builder::IncrementalTreeBuilder;

// ===== 进度事件处理 =====

/// 处理引擎进度事件
pub(crate) fn handle_progress(app: &mut App, evt: ProgressEvent) {
    match evt {
        ProgressEvent::Scanning { path } => {
            if let AppState::Scanning {
                ref mut progress_text,
                ..
            } = app.state
            {
                // 显示相对 home 的当前扫描目录（末尾若干字符），随遍历实时移动，
                // 让用户看到"正在扫描哪里"而非静止的顶层名。渲染节流已限制刷新频率，
                // 不会狂闪；相比只显示顶层名，深层路径更能传达"在动"。
                let home = platform::get_home_dir();
                // home 之下的路径显示为 ~/…；不在 home 下的绝对路径（如 /Applications）
                // 原样显示，避免误拼成 "~//Applications"。
                let (prefix, s) = match path.strip_prefix(&home) {
                    Ok(rel) => ("~/", rel.to_string_lossy()),
                    Err(_) => ("", path.to_string_lossy()),
                };
                let char_count = s.chars().count();
                let new_text = if char_count > 46 {
                    let tail: String = s.chars().skip(char_count - 43).collect();
                    format!("当前: …{tail}")
                } else {
                    format!("当前: {prefix}{s}")
                };
                if *progress_text != new_text {
                    *progress_text = new_text;
                }
            }
        }
        ProgressEvent::RuleProgress {
            current,
            total,
            name,
        } => {
            if let AppState::Scanning {
                ref mut rule_current,
                ref mut rule_total,
                ref mut rule_name,
                ..
            } = app.state
            {
                *rule_current = current;
                *rule_total = total;
                *rule_name = name;
            }
        }
        ProgressEvent::Found {
            category,
            path,
            size,
            safety,
            impact,
            recovery,
            preselect,
        } => {
            // 仅在扫描态接受 Found：防止已取消/已结束扫描的残留事件在返回菜单等
            // 非扫描态重建 scan_result（会让下个命令看到上个命令的检测结果）。
            if !matches!(app.state, AppState::Scanning { .. }) {
                return;
            }
            // __analyze_tree__ 路径已废弃，但保留兼容处理避免数据丢失
            if category == "__analyze_tree__" {
                return;
            }
            use mc_core::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};

            if app.scan_result.is_none() {
                app.scan_result = Some(ScanResult::default());
            }

            // Clean 流式上报同一 (category, root.path) 的增量，此处按 path 合并到既有聚合项，
            // 避免重复插入。merged=true 表示只是给既有项累加 size（不新增计数）。
            //
            // 只比对**末项**而非线性查找全部 items：scan_with_rules 顺序处理各根，某分类的
            // 当前可合并项恒为最后压入的那一项（新根到来才追加新末项）。Purge/Uninstall 的
            // 每个 Found 都是唯一路径、末项必不匹配→直接 push。由此把每事件的合并从 O(n)
            // 降为 O(1)，避免单分类累积上千项时的 O(n²) 主线程卡顿。
            // KTD3：预选（selected = safety != Risky && preselect）在**新项首次插入**时
            // 就地播种到 marked，让扫描期的手动勾选/取消与预选累积到同一集合，
            // 完成时 init_results 不再重播种冲掉。合并累加分支（既有项）不重复播种。
            let should_preselect = preselect && safety != SafetyLevel::Risky;
            let mut merged = false;
            let mut to_preselect: Option<PathBuf> = None;
            if let Some(result) = app.scan_result.as_mut() {
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    if let Some(existing) = cat.items.last_mut().filter(|it| it.path == path) {
                        existing.size += size;
                        cat.total_size += size;
                        merged = true;
                    } else {
                        if should_preselect {
                            to_preselect = Some(path.clone());
                        }
                        cat.file_count += 1;
                        cat.total_size += size;
                        cat.items.push(
                            ScanItem::new(path, size, safety, category.clone())
                                .with_evidence(impact, recovery)
                                .with_preselect(preselect),
                        );
                    }
                } else {
                    if should_preselect {
                        to_preselect = Some(path.clone());
                    }
                    result.categories.push(CategoryGroup::new(
                        category.clone(),
                        vec![ScanItem::new(path, size, safety, category.clone())
                            .with_evidence(impact, recovery)
                            .with_preselect(preselect)],
                    ));
                    app.expanded.push(false);
                }
                result.total_size += size;
                if !merged {
                    result.file_count += 1;
                }
            }
            if let Some(p) = to_preselect {
                app.marked.insert(p);
            }
            // 已发现项数/总大小不再单独维护：render_scan_header 直接读 scan_result。
        }
        ProgressEvent::CategoryDone {
            category,
            total_size,
            count,
        } => {
            if let Some(ref mut result) = app.scan_result {
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    cat.total_size = total_size;
                    cat.file_count = count;
                }
            }
        }
        ProgressEvent::Complete => {
            if let AppState::Scanning { .. } = &app.state {
                if app.active_command == Some(ActiveCommand::Analyze) {
                    return;
                }
                if let Some(ref result) = app.scan_result {
                    if result.file_count == 0 {
                        app.state = AppState::Done {
                            message: "未发现可清理的文件。".into(),
                        };
                        return;
                    }
                } else {
                    app.state = AppState::Done {
                        message: "未发现可清理的文件。".into(),
                    };
                    return;
                }
                if let Some(ref mut result) = app.scan_result {
                    result.total_size = result.categories.iter().map(|c| c.total_size).sum();
                    result.file_count = result.categories.iter().map(|c| c.file_count).sum();
                    // 不再 sort_by(name) 重排底层 vec：display 顺序由 build_flat_rows 决定，
                    // 重排会打乱 expanded/marked 的按 cat_idx 对齐，造成完成瞬间展开态跳变。
                }
                app.init_results();
                // 进入 Results 时清除扫描态残留提示（如 Scanning 态按 Space 的 toast，KTD7）。
                app.status_message = None;
                app.state = AppState::Results;
            }
        }
        ProgressEvent::CleaningFile { path } => {
            if let AppState::Cleaning {
                ref mut progress_text,
            } = app.state
            {
                *progress_text = path.display().to_string();
            }
        }
        ProgressEvent::CleaningDone { freed, count, deleted_paths } => {
            if let Some(ret) = app.analyzer_return.take() {
                // 分析器发起的删除：仅剪除成功删除的节点并原地返回，不拆树回菜单
                restore_analyzer_after_delete(app, ret, freed, count, &deleted_paths);
            } else {
                // Results 路径：由暂存待删清单派生成功/失败明细 + 分类小结，Done 屏完整复述（KTD6）。
                let request = std::mem::take(&mut app.clean_request);
                app.done_report =
                    Some(app::DoneReport::from_request(&request, freed, &deleted_paths));
                app.state = AppState::Done { message: String::new() };
            }
        }
        ProgressEvent::Error(msg) => {
            if matches!(app.state, AppState::Scanning { .. } | AppState::Cleaning { .. }) {
                app.state = AppState::Done {
                    message: format!("错误: {msg}"),
                };
            }
        }
        // 权限跳过（#23）：CLI 端会单列「跳过（需授权）」区并引导 mc doctor；
        // TUI 侧的跳过区渲染属独立范围，本批不做，此处显式 no-op（收下事件不改状态，
        // 保持穷尽 match 与取消/契约语义不变）。
        ProgressEvent::SkippedNoPermission { .. } => {}
    }
}

// ===== Analyze 事件处理（拆分为 entry 和 finished 解决借用冲突）=====

/// 处理 `AnalyzeEvent::Entry` 和 Progress（不修改 `analyze_rx`）
pub(crate) fn handle_analyze_entry(
    app: &mut App,
    evt: AnalyzeEvent,
    builder: &mut IncrementalTreeBuilder,
) {
    match evt {
        AnalyzeEvent::Entry {
            name,
            path,
            size,
            is_file,
        } => {
            if let AppState::AnalyzingLive {
                tree_root,
                file_count,
                total_size,
                user_navigated,
                cursor,
                ..
            } = &mut app.state
            {
                let _ = builder.integrate_entry(tree_root, name, path, size, is_file);
                if is_file {
                    *file_count += 1;
                    *total_size += size;
                }
                if !*user_navigated {
                    // 跟随最大项：显示层按 size 降序排列，最大项恒在显示序 0，
                    // 故未手动导航时把光标钉在 0 即自动跟随当前最大子项。
                    *cursor = 0;
                }
            }
        }
        AnalyzeEvent::Progress { .. } => { /* 统计已在 Entry 中更新 */ }
        AnalyzeEvent::Finished => {
            // Finished 应由 handle_analyze_finished 处理，此处不应到达
        }
    }
}

/// 从 `AnalyzingLive` 过渡到 Sorting：提取树、启动后台排序线程、清理 analyze 资源
pub(crate) fn transition_to_sorting(
    app: &mut App,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    // 进入新 AppState 时主动清除瞬时提示：AnalyzingLive 的"扫描进行中不可标记/删除"toast
    // 不应残留到 Sorting/Analyzing（KTD7）。放在转换点而非按键处，故静态态提示仍走"下次按键清除"。
    app.status_message = None;
    if let AppState::AnalyzingLive { .. } = &app.state {
        let old = std::mem::replace(&mut app.state, AppState::Menu);
        if let AppState::AnalyzingLive { tree_root, .. } = old {
            app.state = AppState::Sorting;

            let (tx, rx) = crossbeam_channel::bounded::<DirNode>(1);
            *sort_rx = Some(rx);

            thread::spawn(move || {
                let mut tree = tree_root;
                IncrementalTreeBuilder::finalize(&mut tree);
                let _ = tx.send(tree);
            });
        }
    }
    app.active_command = None;
    *analyze_rx = None;
    *tree_builder = None;
}

pub(crate) fn handle_analyze_finished(
    app: &mut App,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    transition_to_sorting(app, analyze_rx, tree_builder, sort_rx);
}

/// 取消分析扫描并干净地返回菜单（不保留部分树、不进入排序浏览器）。
pub(crate) fn cancel_analyze_to_menu(
    app: &mut App,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    app.back_to_menu();
    *analyze_rx = None; // drop Receiver 让扫描线程 send 失败而退出
    *tree_builder = None;
    *sort_rx = None;
}

/// 子界面按 q：返回菜单。若存在已标记项，先置 `pending_leave` 提示，再按一次才真正返回，
/// 避免手滑一个 q 丢掉辛苦标记（对齐 dua `pending_exit`）。按状态选择干净的收尾方式。
pub(crate) fn request_leave_to_menu(
    app: &mut App,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    if !app.marked.is_empty() && !app.pending_leave {
        app.pending_leave = true;
        app.status_message = Some(format!(
            "已标记 {} 项未删除，再按一次 q 放弃并返回菜单",
            app.marked.len()
        ));
        return;
    }
    match app.state {
        AppState::AnalyzingLive { .. } => {
            cancel_analyze_to_menu(app, analyze_rx, tree_builder, sort_rx);
        }
        AppState::Scanning { .. } => {
            app.cancel_flag.store(true, Ordering::Relaxed);
            app.back_to_menu();
            app.cancel_flag = Arc::new(AtomicBool::new(false));
        }
        AppState::Sorting => {
            *sort_rx = None;
            app.back_to_menu();
        }
        _ => app.back_to_menu(),
    }
}
