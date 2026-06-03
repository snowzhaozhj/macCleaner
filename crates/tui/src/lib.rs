mod app;
mod event;
mod reporter;
mod ui;

use app::{ActiveCommand, App, AppState, FlatRow};
use event::{AppEvent, EventHandler};
use reporter::TuiReporter;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use humansize::{format_size, DECIMAL};
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, DirNode};
use mc_core::platform;
use mc_core::progress::{ProgressEvent, ProgressReporter};
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

pub fn run() -> Result<()> {
    // 设置 panic hook：确保终端在 panic 时恢复
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // 初始化终端
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    // 恢复终端
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_app(terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<io::Stdout>>) -> Result<()> {
    let mut app = App::new();
    let events = EventHandler::new();
    let pending_tree: Arc<Mutex<Option<DirNode>>> = Arc::new(Mutex::new(None));

    loop {
        // 检查是否有后台完成的目录树
        if matches!(app.state, AppState::Scanning { .. })
            && app.active_command == Some(ActiveCommand::Analyze)
        {
            let mut lock = pending_tree.lock().unwrap();
            if let Some(tree) = lock.take() {
                // 恢复之前导航保存的面包屑（如果有）
                let (breadcrumb, _old_node) = app
                    .analyze_breadcrumb_stash
                    .take()
                    .map(|(mut bc, old)| { bc.push(old); (bc, None::<DirNode>) })
                    .unwrap_or_else(|| (Vec::new(), None));
                app.state = AppState::Analyzing {
                    node: tree,
                    breadcrumb,
                    cursor: 0,
                    marked_for_delete: Vec::new(),
                };
            }
        }

        // 渲染
        terminal.draw(|f| ui::draw(f, &app))?;

        // 等待事件
        match events.next()? {
            AppEvent::Key(key) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
                {
                    break;
                }
                handle_key(&mut app, key.code, &events, &pending_tree);
            }
            AppEvent::Progress(evt) => {
                handle_progress(&mut app, evt);
            }
            AppEvent::Tick => {}
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// 处理键盘输入
fn handle_key(app: &mut App, key: KeyCode, events: &EventHandler, pending_tree: &Arc<Mutex<Option<DirNode>>>) {
    match &app.state {
        AppState::Menu => handle_menu_key(app, key, events, pending_tree),
        AppState::Scanning { .. } => {
            // 扫描中不响应按键（除了 Ctrl+C 已在外层处理）
        }
        AppState::Results => handle_results_key(app, key, events),
        AppState::Confirming => handle_confirm_key(app, key, events),
        AppState::Cleaning { .. } => {
            // 清理中不响应按键
        }
        AppState::Done { .. } => handle_done_key(app, key),
        AppState::Analyzing { .. } => handle_analyzer_key(app, key, pending_tree),
    }
}

/// 菜单页键盘处理
fn handle_menu_key(app: &mut App, key: KeyCode, events: &EventHandler, pending_tree: &Arc<Mutex<Option<DirNode>>>) {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.menu_index > 0 {
                app.menu_index -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.menu_index < 3 {
                app.menu_index += 1;
            }
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
            start_command(app, cmd, events, pending_tree);
        }
        _ => {}
    }
}

/// 启动命令执行
fn start_command(app: &mut App, cmd: ActiveCommand, events: &EventHandler, pending_tree: &Arc<Mutex<Option<DirNode>>>) {
    match cmd {
        ActiveCommand::Clean => {
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                found_count: 0,
                found_size: 0,
            };
            let tx = events.progress_sender();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx);
                match Engine::scan_clean(&reporter) {
                    Ok(_result) => {
                        // Complete 事件已由引擎发送
                    }
                    Err(e) => {
                        reporter.on_event(ProgressEvent::Error(e.to_string()));
                    }
                }
            });
        }
        ActiveCommand::Purge => {
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                found_count: 0,
                found_size: 0,
            };
            let tx = events.progress_sender();
            let path = app.purge_path.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx);
                match Engine::scan_purge(&path, &reporter) {
                    Ok(_result) => {}
                    Err(e) => {
                        reporter.on_event(ProgressEvent::Error(e.to_string()));
                    }
                }
            });
        }
        ActiveCommand::Analyze => {
            app.state = AppState::Scanning {
                progress_text: "正在分析磁盘（单次并行遍历）...".into(),
                found_count: 0,
                found_size: 0,
            };
            let tree_slot = pending_tree.clone();
            let home = platform::get_home_dir();
            thread::spawn(move || {
                match build_dir_tree(&home, 1) {
                    Ok(tree) => {
                        if let Ok(mut slot) = tree_slot.lock() {
                            *slot = Some(tree);
                        }
                    }
                    Err(_) => {}
                }
            });
        }
        ActiveCommand::Uninstall => {
            // Uninstall 使用同步扫描应用列表，然后跳转到结果页
            use mc_core::app_resolver::AppResolver;
            use mc_core::models::{CategoryGroup, ScanItem, SafetyLevel, ScanResult};

            let apps = AppResolver::list_apps();
            if apps.is_empty() {
                app.state = AppState::Done {
                    message: "未发现已安装的应用。".into(),
                };
                return;
            }

            // 将应用列表转换为 ScanResult 格式
            let items: Vec<ScanItem> = apps
                .iter()
                .map(|a| {
                    ScanItem::new(
                        a.path.clone(),
                        a.size,
                        SafetyLevel::Moderate,
                        "已安装应用".into(),
                    )
                })
                .collect();

            let cat = CategoryGroup::new("已安装应用".into(), items);
            let result = ScanResult::from_categories(vec![cat]);
            app.scan_result = Some(result);
            app.init_results();
            app.state = AppState::Results;
        }
    }
}

/// 处理引擎进度事件
fn handle_progress(app: &mut App, evt: ProgressEvent) {
    match evt {
        ProgressEvent::Scanning { path } => {
            if let AppState::Scanning {
                ref mut progress_text,
                ..
            } = app.state
            {
                *progress_text = path.display().to_string();
            }
        }
        ProgressEvent::Found {
            category,
            path,
            size,
        } => {
            if category == "__analyze_tree__" {
                // analyze 特殊标记，忽略
                return;
            }
            if let AppState::Scanning {
                ref mut found_count,
                ref mut found_size,
                ..
            } = app.state
            {
                *found_count += 1;
                *found_size += size;
            }

            // 增量地将找到的项目添加到 scan_result
            use mc_core::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
            let item = ScanItem::new(path, size, SafetyLevel::Safe, category.clone());

            if app.scan_result.is_none() {
                app.scan_result = Some(ScanResult::default());
            }
            if let Some(ref mut result) = app.scan_result {
                // 查找或创建 category
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    cat.file_count += 1;
                    cat.total_size += size;
                    cat.items.push(item);
                } else {
                    result
                        .categories
                        .push(CategoryGroup::new(category, vec![item]));
                }
                result.file_count += 1;
                result.total_size += size;
            }
        }
        ProgressEvent::CategoryDone {
            category,
            total_size,
            count,
        } => {
            // 用引擎汇总的信息更新 category（修正增量累加的差异）
            if let Some(ref mut result) = app.scan_result {
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    cat.total_size = total_size;
                    cat.file_count = count;
                }
            }
        }
        ProgressEvent::Complete => {
            // 扫描完成，切换到结果页
            match &app.state {
                AppState::Scanning { .. } => {
                    if app.active_command == Some(ActiveCommand::Analyze) {
                        // Analyze 不使用异步扫描流程
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
                    // 重新计算 ScanResult 的 total（以 CategoryDone 为准）
                    if let Some(ref mut result) = app.scan_result {
                        result.total_size = result.categories.iter().map(|c| c.total_size).sum();
                        result.file_count = result.categories.iter().map(|c| c.file_count).sum();
                        result.categories.sort_by(|a, b| a.name.cmp(&b.name));
                    }
                    app.init_results();
                    app.state = AppState::Results;
                }
                _ => {}
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
        ProgressEvent::CleaningDone { freed, count } => {
            app.state = AppState::Done {
                message: format!(
                    "清理完成！已清理 {} 个文件，释放 {}",
                    count,
                    format_size(freed, DECIMAL)
                ),
            };
        }
        ProgressEvent::Error(msg) => {
            app.state = AppState::Done {
                message: format!("错误: {}", msg),
            };
        }
    }
}

/// 结果页键盘处理
fn handle_results_key(app: &mut App, key: KeyCode, _events: &EventHandler) {
    let flat_rows = app.build_flat_rows();
    let row_count = flat_rows.len();

    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.back_to_menu();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.result_cursor > 0 {
                app.result_cursor -= 1;
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if row_count > 0 && app.result_cursor < row_count - 1 {
                app.result_cursor += 1;
            }
        }
        KeyCode::Char(' ') => {
            // 切换选中状态
            if let Some(row) = flat_rows.get(app.result_cursor) {
                let row = row.clone();
                app.toggle_selection(&row);
            }
        }
        KeyCode::Tab => {
            // 展开/折叠
            if let Some(FlatRow::Category { cat_idx, .. }) = flat_rows.get(app.result_cursor) {
                app.toggle_expand(*cat_idx);
            }
        }
        KeyCode::Char('a') => {
            app.select_all_safe();
        }
        KeyCode::Enter => {
            let (count, _) = app.selected_summary();
            if count > 0 {
                app.state = AppState::Confirming;
            }
        }
        _ => {}
    }
}

/// 确认页键盘处理
fn handle_confirm_key(app: &mut App, key: KeyCode, events: &EventHandler) {
    match key {
        KeyCode::Enter | KeyCode::Char('y') => {
            // 执行清理
            app.state = AppState::Cleaning {
                progress_text: "准备清理...".into(),
            };

            // 收集选中的项目路径和大小
            let items: Vec<(std::path::PathBuf, u64)> = if let Some(ref result) = app.scan_result {
                result
                    .selected_items()
                    .iter()
                    .map(|i| (i.path.clone(), i.size))
                    .collect()
            } else {
                Vec::new()
            };

            let tx = events.progress_sender();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx);
                // 重新构建 ScanItem 列表（因为跨线程不能直接引用 result）
                use mc_core::models::{ScanItem, SafetyLevel};
                let scan_items: Vec<ScanItem> = items
                    .iter()
                    .map(|(path, size)| {
                        ScanItem::new(path.clone(), *size, SafetyLevel::Safe, "".into())
                    })
                    .collect();
                let refs: Vec<&ScanItem> = scan_items.iter().collect();

                match Engine::clean(&refs, DeleteMode::Trash, &reporter) {
                    Ok(_report) => {
                        // CleaningDone 事件已由引擎发送
                    }
                    Err(e) => {
                        reporter.on_event(ProgressEvent::Error(e.to_string()));
                    }
                }
            });
        }
        KeyCode::Esc | KeyCode::Char('n') => {
            app.state = AppState::Results;
        }
        _ => {}
    }
}

/// 完成页键盘处理
fn handle_done_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter => {
            app.back_to_menu();
        }
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        _ => {}
    }
}

/// 磁盘分析器键盘处理
fn handle_analyzer_key(app: &mut App, key: KeyCode, pending_tree: &Arc<Mutex<Option<DirNode>>>) {
    // 先提取需要的信息，避免借用冲突
    let action = match &mut app.state {
        AppState::Analyzing {
            node,
            breadcrumb,
            cursor,
            marked_for_delete,
        } => {
            match key {
                KeyCode::Char('q') => Some(AnalyzerAction::Quit),
                KeyCode::Up | KeyCode::Char('k') => {
                    if *cursor > 0 {
                        *cursor -= 1;
                    }
                    None
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !node.children.is_empty() && *cursor < node.children.len() - 1 {
                        *cursor += 1;
                    }
                    None
                }
                KeyCode::Enter => {
                    if let Some(child) = node.children.get(*cursor) {
                        if !child.is_file {
                            Some(AnalyzerAction::Enter(child.path.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                KeyCode::Backspace | KeyCode::Esc => {
                    if let Some(parent) = breadcrumb.pop() {
                        *node = parent;
                        *cursor = 0;
                        None
                    } else {
                        Some(AnalyzerAction::Quit)
                    }
                }
                KeyCode::Char('d') => {
                    if let Some(child) = node.children.get(*cursor) {
                        let path = child.path.clone();
                        if let Some(pos) = marked_for_delete.iter().position(|p| *p == path) {
                            marked_for_delete.remove(pos);
                        } else {
                            marked_for_delete.push(path);
                        }
                    }
                    None
                }
                _ => None,
            }
        }
        _ => None,
    };

    match action {
        Some(AnalyzerAction::Quit) => {
            app.back_to_menu();
        }
        Some(AnalyzerAction::Enter(child_path)) => {
            // 保存当前状态到面包屑，切换到扫描状态
            if let AppState::Analyzing { node, breadcrumb, .. } = std::mem::replace(
                &mut app.state,
                AppState::Scanning {
                    progress_text: format!("正在分析 {} ...", child_path.display()),
                    found_count: 0,
                    found_size: 0,
                },
            ) {
                app.analyze_breadcrumb_stash = Some((breadcrumb, node));
            }
            app.active_command = Some(ActiveCommand::Analyze);
            let tree_slot = pending_tree.clone();
            thread::spawn(move || {
                if let Ok(new_node) = build_dir_tree(&child_path, 1) {
                    if let Ok(mut slot) = tree_slot.lock() {
                        *slot = Some(new_node);
                    }
                }
            });
        }
        None => {}
    }
}

enum AnalyzerAction {
    Quit,
    Enter(std::path::PathBuf),
}

/// 构建目录树：单次 jwalk 遍历，边走边累加大小（类似 dua-cli 的方式）
fn build_dir_tree(path: &Path, _max_depth: usize) -> Result<DirNode> {
    use std::collections::HashMap;

    let root_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());

    let root_len = path.components().count();

    let mut child_sizes: HashMap<PathBuf, u64> = HashMap::new();
    let mut child_is_file: HashMap<PathBuf, bool> = HashMap::new();

    let walker = jwalk::WalkDir::new(path)
        .skip_hidden(true)
        .follow_links(false);

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        let components: Vec<_> = entry_path.components().collect();
        let depth = components.len() - root_len;

        if depth == 0 {
            continue;
        }

        // 获取直接子项的路径（根目录下的第一层）
        let child_path: PathBuf = components[..root_len + 1].iter().collect();

        if entry.file_type().is_file() {
            if let Ok(meta) = std::fs::symlink_metadata(&entry_path) {
                let size = meta.len();
                if depth == 1 {
                    // 直接子文件
                    child_is_file.insert(child_path.clone(), true);
                    *child_sizes.entry(child_path).or_insert(0) += size;
                } else {
                    // 子目录内的文件 → 累加到对应的直接子目录
                    child_is_file.entry(child_path.clone()).or_insert(false);
                    *child_sizes.entry(child_path).or_insert(0) += size;
                }
            }
        } else if depth == 1 && entry.file_type().is_dir() {
            // 确保空目录也出现
            child_is_file.entry(child_path.clone()).or_insert(false);
            child_sizes.entry(child_path).or_insert(0);
        }
    }

    // 构建子节点
    let mut children: Vec<DirNode> = Vec::new();

    for (child_path, size) in &child_sizes {
        let name = child_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let is_file = child_is_file.get(child_path).copied().unwrap_or(false);

        if is_file {
            children.push(DirNode::new_file(child_path.clone(), name, *size));
        } else {
            let mut node = DirNode::new_dir(child_path.clone(), name);
            node.size = *size;
            children.push(node);
        }
    }

    children.sort_by(|a, b| b.size.cmp(&a.size));

    let total_size: u64 = children.iter().map(|c| c.size).sum();
    let mut root = DirNode::new_dir(path.to_path_buf(), root_name);
    root.size = total_size;
    root.children = children;

    Ok(root)
}
