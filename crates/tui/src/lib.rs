mod app;
mod event;
mod reporter;
mod ui;

use app::{ActiveCommand, App, AppState, FlatRow};
use event::EventHandler;
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
use std::io::{self, stdout, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

    let backend = ratatui::backend::CrosstermBackend::new(BufWriter::with_capacity(8192, stdout()));
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    // 恢复终端
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn needs_animation(app: &App) -> bool {
    matches!(
        app.state,
        AppState::Scanning { .. } | AppState::Cleaning { .. }
    )
}

fn run_app(terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<BufWriter<io::Stdout>>>) -> Result<()> {
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
                app.analyze_preview = None;
                app.state = AppState::Analyzing {
                    tree_root: Arc::new(tree),
                    nav_path: Vec::new(),
                    cursor: 0,
                    marked_for_delete: Vec::new(),
                    cursor_stack: Vec::new(),
                };
            }
        }

        // 渲染
        terminal.draw(|f| ui::draw(f, &app))?;

        // 事件驱动：动画状态使用超时，静态状态纯阻塞
        if needs_animation(&app) {
            // 动画状态：100ms 超时驱动 spinner 刷新
            crossbeam_channel::select! {
                recv(events.key_rx) -> key => {
                    if let Ok(key) = key {
                        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                            break;
                        }
                        handle_key(&mut app, key.code, &events, &pending_tree);
                    }
                }
                recv(events.progress_rx) -> evt => {
                    if let Ok(evt) = evt {
                        handle_progress(&mut app, evt);
                    }
                }
                default(std::time::Duration::from_millis(100)) => {
                    // 超时：仅用于刷新 spinner 动画，循环回到 draw
                }
            }
        } else {
            // 静态状态：纯阻塞等待事件，零 CPU 开销
            crossbeam_channel::select! {
                recv(events.key_rx) -> key => {
                    if let Ok(key) = key {
                        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                            break;
                        }
                        handle_key(&mut app, key.code, &events, &pending_tree);
                    }
                }
                recv(events.progress_rx) -> evt => {
                    if let Ok(evt) = evt {
                        handle_progress(&mut app, evt);
                    }
                }
            }
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
            match key {
                KeyCode::Esc => {
                    app.cancel_flag.store(true, Ordering::Relaxed);
                    app.state = AppState::Menu;
                    app.active_command = None;
                    app.scan_result = None;
                    app.expanded.clear();
                    app.cancel_flag = Arc::new(AtomicBool::new(false));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if app.result_cursor > 0 {
                        app.result_cursor -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let row_count = app.build_flat_rows().len();
                    if row_count > 0 && app.result_cursor < row_count - 1 {
                        app.result_cursor += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    let flat_rows = app.build_flat_rows();
                    if let Some(row) = flat_rows.get(app.result_cursor) {
                        let row = row.clone();
                        app.toggle_selection(&row);
                    }
                }
                KeyCode::Tab => {
                    let flat_rows = app.build_flat_rows();
                    if let Some(FlatRow::Category { cat_idx, .. }) = flat_rows.get(app.result_cursor) {
                        app.toggle_expand(*cat_idx);
                    }
                }
                KeyCode::Char('a') => {
                    app.select_all_safe();
                }
                _ => {}
            }
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
            app.cancel_flag = Arc::new(AtomicBool::new(false));
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                found_count: 0,
                found_size: 0,
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tx = events.progress_sender();
            let cancel = app.cancel_flag.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
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
            app.cancel_flag = Arc::new(AtomicBool::new(false));
            app.state = AppState::Scanning {
                progress_text: "准备扫描...".into(),
                found_count: 0,
                found_size: 0,
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tx = events.progress_sender();
            let path = app.purge_path.clone();
            let cancel = app.cancel_flag.clone();
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
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
                rule_current: 0,
                rule_total: 0,
                rule_name: String::new(),
            };
            let tree_slot = pending_tree.clone();
            let home = platform::get_home_dir();
            let tx = events.progress_sender();
            thread::spawn(move || {
                match build_dir_tree(&home, Some(&tx)) {
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
        } => {
            if category == "__analyze_tree__" {
                if let AppState::Scanning {
                    ref mut found_count,
                    ref mut found_size,
                    ..
                } = app.state
                {
                    *found_count += 1;
                    *found_size += size;
                }
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

            use mc_core::models::{CategoryGroup, ScanItem, ScanResult};
            let item = ScanItem::new(path, size, safety, category.clone());

            if app.scan_result.is_none() {
                app.scan_result = Some(ScanResult::default());
            }
            if let Some(ref mut result) = app.scan_result {
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    cat.file_count += 1;
                    cat.total_size += size;
                    cat.items.push(item);
                } else {
                    result
                        .categories
                        .push(CategoryGroup::new(category, vec![item]));
                    app.expanded.push(false);
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
        ProgressEvent::AnalyzeSnapshot { tree } => {
            if app.active_command == Some(ActiveCommand::Analyze) {
                app.analyze_preview = Some(tree);
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
            let cancel = Arc::new(AtomicBool::new(false));
            thread::spawn(move || {
                let reporter = TuiReporter::new(tx, cancel);
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

/// 获取 nav_path 指向的当前节点
fn resolve_nav_node<'a>(root: &'a DirNode, nav_path: &[usize]) -> &'a DirNode {
    let mut node = root;
    for &idx in nav_path {
        node = &node.children[idx];
    }
    node
}

/// 磁盘分析器键盘处理（纯内存导航，不做 I/O）
fn handle_analyzer_key(app: &mut App, key: KeyCode, _pending_tree: &Arc<Mutex<Option<DirNode>>>) {
    if let AppState::Analyzing {
        tree_root,
        nav_path,
        cursor,
        marked_for_delete,
        cursor_stack,
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        match key {
            KeyCode::Char('q') => {
                if nav_path.is_empty() {
                    app.back_to_menu();
                } else {
                    // q 在子目录中也返回菜单
                    app.back_to_menu();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *cursor > 0 {
                    *cursor -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 {
                    *cursor += 1;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(child) = current_node.children.get(*cursor) {
                    if !child.is_file && !child.children.is_empty() {
                        cursor_stack.push(*cursor);
                        nav_path.push(*cursor);
                        *cursor = 0;
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if nav_path.is_empty() {
                    app.back_to_menu();
                } else {
                    nav_path.pop();
                    *cursor = cursor_stack.pop().unwrap_or(0);
                }
            }
            KeyCode::Char('d') => {
                if let Some(child) = current_node.children.get(*cursor) {
                    let path = child.path.clone();
                    if let Some(pos) = marked_for_delete.iter().position(|p| *p == path) {
                        marked_for_delete.remove(pos);
                    } else {
                        marked_for_delete.push(path);
                    }
                }
            }
            _ => {}
        }
    }
}

/// 构建完整递归目录树：单次 jwalk 遍历 + 深度栈聚合
fn build_dir_tree(path: &Path, progress_tx: Option<&crossbeam_channel::Sender<ProgressEvent>>) -> Result<DirNode> {
    use std::collections::HashMap;
    use std::time::Instant;

    let root_len = path.components().count();

    let mut dir_children: HashMap<PathBuf, HashMap<String, DirNode>> = HashMap::new();
    dir_children.insert(path.to_path_buf(), HashMap::new());

    // 顶层目录大小追踪（用于渐进式预览）
    let mut toplevel_sizes: HashMap<PathBuf, u64> = HashMap::new();
    let mut last_snapshot = Instant::now();

    let walker = jwalk::WalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(jwalk::Parallelism::RayonNewPool(
            if cfg!(target_os = "macos") { 3 } else { 0 },
        ));

    let mut file_count: usize = 0;
    let mut last_reported_size: u64 = 0;
    let mut total_size: u64 = 0;

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        let entry_path = entry.path();
        let depth = entry_path.components().count() - root_len;
        if depth == 0 {
            continue;
        }

        let parent_path = match entry_path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let name = entry_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        if entry.file_type().is_dir() {
            dir_children.entry(entry_path.clone()).or_insert_with(HashMap::new);
            let siblings = dir_children.entry(parent_path).or_insert_with(HashMap::new);
            siblings
                .entry(name)
                .or_insert_with(|| DirNode::new_dir(entry_path, String::new()));
        } else {
            let size = std::fs::symlink_metadata(&entry_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let siblings = dir_children.entry(parent_path).or_insert_with(HashMap::new);
            siblings
                .entry(name.clone())
                .or_insert_with(|| DirNode::new_file(entry_path.clone(), name, size));

            file_count += 1;
            total_size += size;

            // 追踪顶层目录大小
            if depth >= 1 {
                let toplevel_path: PathBuf = entry_path
                    .components()
                    .take(root_len + 1)
                    .collect();
                *toplevel_sizes.entry(toplevel_path).or_insert(0) += size;
            }

            if file_count % 500 == 0 {
                if let Some(tx) = progress_tx {
                    let _ = tx.send(ProgressEvent::Scanning {
                        path: entry_path,
                    });
                    let size_delta = total_size - last_reported_size;
                    last_reported_size = total_size;
                    let _ = tx.send(ProgressEvent::Found {
                        category: "__analyze_tree__".into(),
                        path: path.to_path_buf(),
                        size: size_delta,
                        safety: mc_core::models::SafetyLevel::Safe,
                    });

                    // 每 250ms 发送一次顶层快照
                    if last_snapshot.elapsed() >= std::time::Duration::from_millis(250) {
                        last_snapshot = Instant::now();
                        let mut children: Vec<DirNode> = toplevel_sizes
                            .iter()
                            .map(|(p, &sz)| {
                                let n = p.file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                let mut node = DirNode::new_dir(p.clone(), n);
                                node.size = sz;
                                node
                            })
                            .collect();
                        children.sort_by(|a, b| b.size.cmp(&a.size));
                        let mut snapshot = DirNode::new_dir(path.to_path_buf(), path.file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_else(|| "~".into()));
                        snapshot.size = total_size;
                        snapshot.children = children;
                        let _ = tx.send(ProgressEvent::AnalyzeSnapshot { tree: snapshot });
                    }
                }
            }
        }
    }

    // 自底向上组装树：按路径深度从深到浅处理
    let mut all_dirs: Vec<PathBuf> = dir_children.keys().cloned().collect();
    all_dirs.sort_by(|a, b| {
        let da = a.components().count();
        let db = b.components().count();
        db.cmp(&da) // 深的先处理
    });

    // node_cache: 完整构建好的 DirNode
    let mut node_cache: HashMap<PathBuf, DirNode> = HashMap::new();

    for dir_path in &all_dirs {
        let children_map = dir_children.remove(dir_path).unwrap_or_default();
        let dir_name = dir_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| dir_path.display().to_string());

        let mut children: Vec<DirNode> = Vec::new();

        for (_name, mut child_node) in children_map {
            if !child_node.is_file {
                // 用已构建好的子树替换
                if let Some(built) = node_cache.remove(&child_node.path) {
                    child_node = built;
                }
            }
            children.push(child_node);
        }

        children.sort_by(|a, b| b.size.cmp(&a.size));
        let total_size: u64 = children.iter().map(|c| c.size).sum();

        let mut node = DirNode::new_dir(dir_path.clone(), dir_name);
        node.size = total_size;
        node.children = children;

        node_cache.insert(dir_path.clone(), node);
    }

    node_cache
        .remove(path)
        .ok_or_else(|| anyhow::anyhow!("failed to build directory tree"))
}
