mod app;
mod event;
mod reporter;
mod throttle;
mod ui;

use app::{ActiveCommand, App, AppState, FlatRow};
use event::EventHandler;
use reporter::TuiReporter;

use anyhow::Result;
use crossbeam_channel::Receiver;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use humansize::{format_size, DECIMAL};
use mc_core::engine::Engine;
use mc_core::models::{DeleteMode, DirNode};
use mc_core::platform;
use mc_core::progress::{AnalyzeEvent, ProgressEvent, ProgressReporter};
use std::io::{self, stdout, BufWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ===== IncrementalTreeBuilder =====

struct IncrementalTreeBuilder {
    /// 深度栈：depth_stack[d] = 深度 d 的当前节点在其父 children 中的索引
    depth_stack: Vec<usize>,
    previous_depth: usize,
}

impl IncrementalTreeBuilder {
    fn new() -> Self {
        Self {
            depth_stack: Vec::new(),
            previous_depth: 0,
        }
    }

    /// 将一个 AnalyzeEvent::Entry 集成到 tree_root。
    /// jwalk 保证 DFS 序，depth 相对于 previous_depth 的关系决定导航方向。
    /// 返回 Option：异常 depth 时返回 None 并跳过，不 panic。
    fn integrate_entry(
        &mut self,
        tree_root: &mut DirNode,
        depth: usize,
        name: String,
        path: PathBuf,
        size: u64,
        is_file: bool,
    ) -> Option<()> {
        // 运行时安全检查
        if depth == 0 || depth > self.previous_depth + 1 {
            return None; // 跳过异常 entry，不 panic
        }

        // 深度导航
        if depth > self.previous_depth {
            if self.previous_depth > 0 {
                // 进入子目录：push 当前深度节点的最后一个 children 索引
                let parent = Self::navigate_to_parent(tree_root, &self.depth_stack, self.previous_depth)?;
                if parent.children.is_empty() {
                    return None;
                }
                self.depth_stack.push(parent.children.len() - 1);
            }
            // previous_depth == 0 时是第一个 entry，直接添加到 tree_root，无需 push
        } else if depth < self.previous_depth {
            // 回退到上层目录
            self.depth_stack.truncate(depth.saturating_sub(1));
        }
        // depth == previous_depth: 不变

        let parent = Self::navigate_to_parent(tree_root, &self.depth_stack, depth)?;
        let new_idx = parent.children.len();
        if is_file {
            parent.children.push(DirNode::new_file(path, name, size));
        } else {
            parent.children.push(DirNode::new_dir(path, name));
        }

        // 更新 depth_stack 以指向新节点
        if self.depth_stack.len() < depth {
            self.depth_stack.push(new_idx);
        } else if let Some(slot) = self.depth_stack.get_mut(depth - 1) {
            *slot = new_idx;
        }

        if is_file && size > 0 {
            Self::propagate_size(tree_root, &self.depth_stack, depth, size);
        }

        self.previous_depth = depth;
        Some(())
    }

    /// 导航到目标深度的父节点，返回 Option 而非裸索引
    fn navigate_to_parent<'a>(
        tree_root: &'a mut DirNode,
        depth_stack: &[usize],
        target_depth: usize,
    ) -> Option<&'a mut DirNode> {
        let mut node = tree_root;
        for i in 0..target_depth.saturating_sub(1) {
            let idx = *depth_stack.get(i)?;
            node = node.children.get_mut(idx)?;
        }
        Some(node)
    }

    /// 向上传播 size 到所有祖先节点
    fn propagate_size(
        tree_root: &mut DirNode,
        depth_stack: &[usize],
        depth: usize,
        size: u64,
    ) {
        tree_root.size += size;
        let mut node = tree_root;
        for i in 0..depth.saturating_sub(1) {
            let idx = match depth_stack.get(i) {
                Some(&idx) => idx,
                None => return, // 栈不一致，停止传播但不 panic
            };
            node = match node.children.get_mut(idx) {
                Some(n) => n,
                None => return,
            };
            node.size += size;
        }
    }

    /// 遍历完成后递归排序所有 children（按 size 降序）
    fn finalize(tree_root: &mut DirNode) {
        fn sort_recursive(node: &mut DirNode) {
            node.children.sort_by_key(|c| std::cmp::Reverse(c.size));
            for child in &mut node.children {
                if !child.is_file {
                    sort_recursive(child);
                }
            }
        }
        sort_recursive(tree_root);
    }
}

// ===== 导航辅助函数 =====

// ===== 核心运行逻辑 =====

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

    let backend =
        ratatui::backend::CrosstermBackend::new(BufWriter::with_capacity(8192, stdout()));
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
        AppState::Scanning { .. } | AppState::Cleaning { .. } | AppState::AnalyzingLive { .. }
    )
}

enum SelectResult {
    Key(crossterm::event::KeyEvent),
    Progress(ProgressEvent),
    Analyze(AnalyzeEvent),
    Timeout,
}

fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<BufWriter<io::Stdout>>>,
) -> Result<()> {
    let mut app = App::new();
    let events = EventHandler::new();

    // Throttle 在动画状态时激活
    let mut throttle: Option<throttle::Throttle> = None;
    // Analyze 专用 channel 和树构建器
    let mut analyze_rx: Option<Receiver<AnalyzeEvent>> = None;
    let mut tree_builder: Option<IncrementalTreeBuilder> = None;

    loop {
        // Throttle 生命周期管理：进入动画状态时创建，离开时 drop
        if needs_animation(&app) {
            if throttle.is_none() {
                throttle = Some(throttle::Throttle::new(Duration::from_millis(200)));
            }
        } else {
            throttle = None;
        }

        if needs_animation(&app) {
            // ---- 动画状态分支 ----
            // 每次 select 处理一个事件（与 dua-cli 相同），channel 背压自然限速

            let select_result = {
                let mut sel = crossbeam_channel::Select::new();
                let key_idx = sel.recv(&events.key_rx);
                let progress_idx = sel.recv(&events.progress_rx);
                let analyze_idx = analyze_rx.as_ref().map(|rx| sel.recv(rx));

                match sel.select_timeout(Duration::from_millis(100)) {
                    Ok(oper) if oper.index() == key_idx => oper
                        .recv(&events.key_rx)
                        .map(SelectResult::Key)
                        .unwrap_or(SelectResult::Timeout),
                    Ok(oper) if oper.index() == progress_idx => oper
                        .recv(&events.progress_rx)
                        .map(SelectResult::Progress)
                        .unwrap_or(SelectResult::Timeout),
                    Ok(oper) if Some(oper.index()) == analyze_idx => {
                        if let Some(ref rx) = analyze_rx {
                            oper.recv(rx)
                                .map(SelectResult::Analyze)
                                .unwrap_or(SelectResult::Analyze(AnalyzeEvent::Finished))
                        } else {
                            SelectResult::Timeout
                        }
                    }
                    _ => SelectResult::Timeout,
                }
            };

            match select_result {
                SelectResult::Key(key) => {
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        break;
                    }
                    handle_key(
                        &mut app,
                        key.code,
                        &events,
                        &mut analyze_rx,
                        &mut tree_builder,
                    );
                    terminal.draw(|f| ui::draw(f, &app))?;
                }
                SelectResult::Progress(evt) => {
                    handle_progress(&mut app, evt);
                    if throttle.as_ref().is_none_or(|t| t.can_update()) {
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                }
                SelectResult::Analyze(evt) => match evt {
                    AnalyzeEvent::Finished => {
                        handle_analyze_finished(
                            &mut app,
                            &mut tree_builder,
                            &mut analyze_rx,
                        );
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                    other => {
                        if let Some(ref mut builder) = tree_builder {
                            handle_analyze_entry(&mut app, other, builder);
                        }
                        if throttle.as_ref().is_none_or(|t| t.can_update()) {
                            terminal.draw(|f| ui::draw(f, &app))?;
                        }
                    }
                },
                SelectResult::Timeout => {
                    if throttle.as_ref().is_none_or(|t| t.can_update()) {
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                }
            }
        } else {
            // 静态状态：先渲染，再纯阻塞等待事件，零 CPU 开销
            terminal.draw(|f| ui::draw(f, &app))?;

            crossbeam_channel::select! {
                recv(events.key_rx) -> key => {
                    if let Ok(key) = key {
                        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                            break;
                        }
                        handle_key(&mut app, key.code, &events, &mut analyze_rx, &mut tree_builder);
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

// ===== 键盘处理 =====

/// 处理键盘输入
fn handle_key(
    app: &mut App,
    key: KeyCode,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    match &app.state {
        AppState::Menu => handle_menu_key(app, key, events, analyze_rx, tree_builder),
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
                KeyCode::Up | KeyCode::Char('k')
                    if app.result_cursor > 0 => {
                        app.result_cursor -= 1;
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
                    if let Some(FlatRow::Category { cat_idx, .. }) =
                        flat_rows.get(app.result_cursor)
                    {
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
        AppState::Analyzing { .. } => handle_analyzer_key(app, key),
        AppState::AnalyzingLive { .. } => {
            handle_analyzer_live_key(app, key, analyze_rx, tree_builder);
        }
    }
}

/// 菜单页键盘处理
fn handle_menu_key(
    app: &mut App,
    key: KeyCode,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
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
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match Engine::scan_clean(&reporter) {
                        Ok(_result) => {}
                        Err(e) => {
                            reporter.on_event(ProgressEvent::Error(e.to_string()));
                        }
                    }
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {:?}", e)));
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
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match Engine::scan_purge(&path, &reporter) {
                        Ok(_result) => {}
                        Err(e) => {
                            reporter.on_event(ProgressEvent::Error(e.to_string()));
                        }
                    }
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {:?}", e)));
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
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "~".into());
            app.state = AppState::AnalyzingLive {
                tree_root: DirNode::new_dir(home.clone(), root_name),
                nav_path: Vec::new(),
                cursor: 0,
                marked_for_delete: Vec::new(),
                cursor_stack: Vec::new(),
                file_count: 0,
                total_size: 0,
            };

            thread::spawn(move || {
                // 用 catch_unwind 包裹遍历，确保 Finished 始终被发送
                let tx_clone = tx.clone();
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        let root_len = home.components().count();
                        let walker = mc_core::create_walker(std::path::Path::new(&home))
                            .process_read_dir(|_depth, _path, _state, children| {
                                mc_core::prefetch_metadata(children);
                            });
                        let mut count = 0u64;
                        let mut total = 0u64;
                        for entry in walker.into_iter().filter_map(|e| e.ok()) {
                            let is_file = !entry.file_type().is_dir();
                            let size = if is_file {
                                entry.client_state.unwrap_or(0)
                            } else {
                                0
                            };
                            let entry_path = entry.path();
                            let depth = entry_path.components().count() - root_len;
                            if depth == 0 {
                                continue;
                            }
                            let name = entry
                                .file_name()
                                .to_string_lossy()
                                .into_owned();
                            if tx_clone
                                .send(AnalyzeEvent::Entry {
                                    depth,
                                    name,
                                    path: entry_path,
                                    size,
                                    is_file,
                                })
                                .is_err()
                            {
                                return; // Receiver 已 drop（用户取消），直接退出
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
                        }
                    }));
                // 无论正常完成还是 panic，都发送 Finished
                let _ = tx.send(AnalyzeEvent::Finished);
                if let Err(e) = result {
                    eprintln!("Analyze 遍历线程 panic: {:?}", e);
                }
            });
        }
        ActiveCommand::Uninstall => {
            // Uninstall 使用同步扫描应用列表，然后跳转到结果页
            use mc_core::app_resolver::AppResolver;
            use mc_core::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};

            let apps = AppResolver::list_apps();
            if apps.is_empty() {
                app.state = AppState::Done {
                    message: "未发现已安装的应用。".into(),
                };
                return;
            }

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

// ===== 进度事件处理 =====

/// 处理引擎进度事件
fn handle_progress(app: &mut App, evt: ProgressEvent) {
    match evt {
        ProgressEvent::Scanning { path } => {
            if let AppState::Scanning {
                ref mut progress_text,
                ..
            } = app.state
            {
                // 提取顶层目录名（低频变化），不再显示快速闪烁的完整路径
                let home = platform::get_home_dir();
                let home_depth = home.components().count();
                let toplevel = path
                    .components()
                    .nth(home_depth)
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .unwrap_or_default();
                let new_text = format!("当前: {}", toplevel);
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
            ..
        } => {
            // __analyze_tree__ 路径已废弃，但保留兼容处理避免数据丢失
            if category == "__analyze_tree__" {
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
                    result.categories.sort_by(|a, b| a.name.cmp(&b.name));
                }
                app.init_results();
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
            if matches!(app.state, AppState::Scanning { .. } | AppState::Cleaning { .. }) {
                app.state = AppState::Done {
                    message: format!("错误: {}", msg),
                };
            }
        }
    }
}

// ===== Analyze 事件处理（拆分为 entry 和 finished 解决借用冲突）=====

/// 处理 AnalyzeEvent::Entry 和 Progress（不修改 analyze_rx）
fn handle_analyze_entry(
    app: &mut App,
    evt: AnalyzeEvent,
    builder: &mut IncrementalTreeBuilder,
) {
    match evt {
        AnalyzeEvent::Entry {
            depth,
            name,
            path,
            size,
            is_file,
        } => {
            if let AppState::AnalyzingLive {
                tree_root,
                file_count,
                total_size,
                ..
            } = &mut app.state
            {
                let _ = builder.integrate_entry(tree_root, depth, name, path, size, is_file);
                if is_file {
                    *file_count += 1;
                    *total_size += size;
                }
            }
        }
        AnalyzeEvent::Progress { .. } => { /* 统计已在 Entry 中更新 */ }
        AnalyzeEvent::Finished => {
            // Finished 应由 handle_analyze_finished 处理，此处不应到达
        }
    }
}

/// 处理 Finished 事件：完成树构建，切换到 Analyzing 状态
fn handle_analyze_finished(
    app: &mut App,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
) {
    if let AppState::AnalyzingLive { .. } = &app.state {
        let old = std::mem::replace(&mut app.state, AppState::Menu);
        if let AppState::AnalyzingLive {
            mut tree_root,
            nav_path: _,
            cursor: _,
            marked_for_delete,
            ..
        } = old
        {
            IncrementalTreeBuilder::finalize(&mut tree_root);

            app.state = AppState::Analyzing {
                tree_root: Arc::new(tree_root),
                nav_path: Vec::new(),
                cursor: 0,
                marked_for_delete,
                cursor_stack: Vec::new(),
            };
        }
    }
    app.active_command = None;
    *analyze_rx = None;
    *tree_builder = None;
}

/// 原子性中止 Analyze：清理三个资源
fn abort_analyze(
    app: &mut App,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    app.state = AppState::Menu;
    app.active_command = None; // 必须重置，否则残留 Some(Analyze) 污染后续流程
    *analyze_rx = None; // drop Receiver -> 后台线程 send 失败退出
    *tree_builder = None; // drop IncrementalTreeBuilder
}

// ===== 各状态键盘处理 =====

/// 结果页键盘处理
fn handle_results_key(app: &mut App, key: KeyCode, _events: &EventHandler) {
    let flat_rows = app.build_flat_rows();
    let row_count = flat_rows.len();

    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.back_to_menu();
        }
        KeyCode::Up | KeyCode::Char('k')
            if app.result_cursor > 0 => {
                app.result_cursor -= 1;
            }
        KeyCode::Down | KeyCode::Char('j')
            if row_count > 0 && app.result_cursor < row_count - 1 => {
                app.result_cursor += 1;
            }
        KeyCode::Char(' ') => {
            if let Some(row) = flat_rows.get(app.result_cursor) {
                let row = row.clone();
                app.toggle_selection(&row);
            }
        }
        KeyCode::Tab => {
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
            app.state = AppState::Cleaning {
                progress_text: "准备清理...".into(),
            };

            let items: Vec<(PathBuf, u64)> = if let Some(ref result) = app.scan_result {
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
                use mc_core::models::{SafetyLevel, ScanItem};
                let scan_items: Vec<ScanItem> = items
                    .iter()
                    .map(|(path, size)| {
                        ScanItem::new(path.clone(), *size, SafetyLevel::Safe, "".into())
                    })
                    .collect();
                let refs: Vec<&ScanItem> = scan_items.iter().collect();

                match Engine::clean(&refs, DeleteMode::Trash, &reporter) {
                    Ok(_report) => {}
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
    ui::analyzer::resolve_node(root, nav_path)
}

/// 磁盘分析器键盘处理（Analyzing 状态，完成后的纯内存导航）
fn handle_analyzer_key(app: &mut App, key: KeyCode) {
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
                app.back_to_menu();
            }
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
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

/// AnalyzingLive 状态键盘处理（增量构建中的可导航界面）
fn handle_analyzer_live_key(
    app: &mut App,
    key: KeyCode,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
) {
    // 先提取需要的字段进行操作
    if let AppState::AnalyzingLive {
        tree_root,
        nav_path,
        cursor,
        marked_for_delete,
        cursor_stack,
        ..
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        match key {
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
                }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(child) = current_node.children.get(*cursor) {
                    // AnalyzingLive 允许进入 children 为空的非文件节点
                    // live 模式下内容会渐进式出现，不需要等 children 非空
                    if !child.is_file {
                        cursor_stack.push(*cursor);
                        nav_path.push(*cursor);
                        *cursor = 0;
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if nav_path.is_empty() {
                    // 根层：取消 Analyze，原子性清理三个资源
                    abort_analyze(app, analyze_rx, tree_builder);
                } else {
                    nav_path.pop();
                    *cursor = cursor_stack.pop().unwrap_or(0);
                }
            }
            KeyCode::Char('q') => {
                abort_analyze(app, analyze_rx, tree_builder);
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
