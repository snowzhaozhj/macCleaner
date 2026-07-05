mod app;
mod event;
mod keymap;
mod reporter;
mod theme;
mod throttle;
mod ui;

use std::collections::HashSet;

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

/// 翻页/Ctrl+d/Ctrl+u 一次移动的行数
const PAGE_STEP: usize = 10;

// ===== IncrementalTreeBuilder =====

struct IncrementalTreeBuilder {
    /// `深度栈：depth_stack`[d] = 深度 d 的当前节点在其父 children 中的索引
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

    /// 将一个 `AnalyzeEvent::Entry` 集成到 `tree_root`。
    /// jwalk 保证 DFS 序，depth 相对于 `previous_depth` 的关系决定导航方向。
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
    // 读取 NO_COLOR 等主题环境变量（一次性）
    theme::init();

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
        AppState::Scanning { .. } | AppState::Cleaning { .. }
        | AppState::AnalyzingLive { .. } | AppState::Sorting
    )
}

enum SelectResult {
    Key(crossterm::event::KeyEvent),
    Progress(ProgressEvent),
    Analyze(AnalyzeEvent),
    SortDone(std::result::Result<DirNode, crossbeam_channel::RecvError>),
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
    // Sort 完成 channel（finalize 在后台线程执行）
    let mut sort_rx: Option<Receiver<DirNode>> = None;

    loop {
        // Throttle 生命周期管理：进入动画状态时创建，离开时 drop
        if needs_animation(&app) {
            if throttle.is_none() {
                // 80ms(~12fps)：显著快于原 200ms(5fps)的顿挫，又不至于让实时列表狂闪
                throttle = Some(throttle::Throttle::new(Duration::from_millis(80)));
                // 进入动画态立即画首帧，消除"按 Enter 后旧界面停留 ~100ms"的卡顿感
                terminal.draw(|f| ui::draw(f, &app))?;
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
                let sort_idx = sort_rx.as_ref().map(|rx| sel.recv(rx));

                match sel.select_timeout(Duration::from_millis(100)) {
                    Ok(oper) if oper.index() == key_idx => oper
                        .recv(&events.key_rx)
                        .map_or(SelectResult::Timeout, SelectResult::Key),
                    Ok(oper) if oper.index() == progress_idx => oper
                        .recv(&events.progress_rx)
                        .map_or(SelectResult::Timeout, SelectResult::Progress),
                    Ok(oper) if Some(oper.index()) == analyze_idx => {
                        if let Some(ref rx) = analyze_rx {
                            oper.recv(rx)
                                .map_or(SelectResult::Analyze(AnalyzeEvent::Finished), SelectResult::Analyze)
                        } else {
                            SelectResult::Timeout
                        }
                    }
                    Ok(oper) if Some(oper.index()) == sort_idx => {
                        if let Some(ref rx) = sort_rx {
                            SelectResult::SortDone(oper.recv(rx))
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
                        key.modifiers,
                        &events,
                        &mut analyze_rx,
                        &mut tree_builder,
                        &mut sort_rx,
                    );
                    terminal.draw(|f| ui::draw(f, &app))?;
                }
                SelectResult::Progress(evt) => {
                    handle_progress(&mut app, evt);
                    if throttle.as_ref().is_none_or(throttle::Throttle::can_update) {
                        app.tick = app.tick.wrapping_add(1);
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                }
                SelectResult::Analyze(evt) => match evt {
                    AnalyzeEvent::Finished => {
                        handle_analyze_finished(
                            &mut app,
                            &mut tree_builder,
                            &mut analyze_rx,
                            &mut sort_rx,
                        );
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                    other => {
                        if let Some(ref mut builder) = tree_builder {
                            handle_analyze_entry(&mut app, other, builder);
                        }
                        if throttle.as_ref().is_none_or(throttle::Throttle::can_update) {
                            app.tick = app.tick.wrapping_add(1);
                            terminal.draw(|f| ui::draw(f, &app))?;
                        }
                    }
                },
                SelectResult::SortDone(result) => {
                    match result {
                        Ok(sorted_tree) => {
                            if let AppState::Sorting =
                                std::mem::replace(&mut app.state, AppState::Menu)
                            {
                                // finalize() 重排了 children，实时态的 discovery-order 索引已失效，
                                // 故 nav_path/cursor 重置为根；cursor_stack 同步清空以维持
                                // cursor_stack.len()==nav_path.len() 不变式（审查 F-low）。
                                app.state = AppState::Analyzing {
                                    tree_root: Arc::new(sorted_tree),
                                    nav_path: Vec::new(),
                                    cursor: 0,
                                    cursor_stack: Vec::new(),
                                };
                            }
                        }
                        Err(_) => {
                            // 排序线程 panic — 回退到 Menu
                            app.state = AppState::Menu;
                        }
                    }
                    sort_rx = None;
                    terminal.draw(|f| ui::draw(f, &app))?;
                }
                SelectResult::Timeout => {
                    if throttle.as_ref().is_none_or(throttle::Throttle::can_update) {
                        app.tick = app.tick.wrapping_add(1);
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
                        handle_key(&mut app, key.code, key.modifiers, &events, &mut analyze_rx, &mut tree_builder, &mut sort_rx);
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
    modifiers: KeyModifiers,
    events: &EventHandler,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    // 过滤输入模式优先：捕获所有字符编辑 filter_query（在 ?/q 拦截之前，避免误触发）
    if app.filter_active {
        match key {
            KeyCode::Esc => {
                app.filter_active = false;
                app.filter_query.clear();
                app.clamp_result_cursor();
            }
            KeyCode::Enter => {
                app.filter_active = false; // 保留过滤词，退出输入模式
            }
            KeyCode::Backspace => {
                app.filter_query.pop();
                app.clamp_result_cursor();
            }
            KeyCode::Char(c) => {
                app.filter_query.push(c);
                app.clamp_result_cursor();
            }
            _ => {}
        }
        return;
    }
    // 删除确认覆盖层优先：Some 时只处理确认/取消并吞键
    if app.confirm_delete.is_some() {
        match key {
            KeyCode::Enter | KeyCode::Char('y') => {
                if let Some(list) = app.confirm_delete.take() {
                    // 分析器发起的删除：删后原地留在树内（暂存树剪枝恢复）；
                    // 其余（Results）：删后走 Done → 菜单。
                    if matches!(app.state, AppState::Analyzing { .. }) {
                        start_cleaning_from_analyzer(app, list, events);
                    } else {
                        start_cleaning(app, list, events);
                    }
                }
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                app.confirm_delete = None;
            }
            _ => {}
        }
        return;
    }
    // 帮助覆盖层优先：打开时任意键关闭并吞掉该键，不透传下层
    if app.show_help {
        app.show_help = false;
        return;
    }
    // ? 打开帮助覆盖层
    if key == KeyCode::Char('?') {
        app.show_help = true;
        return;
    }
    // 每次按键先清除上一次的瞬时提示；pending_leave 仅在连续按 q 时保留
    app.status_message = None;
    if key != KeyCode::Char('q') {
        app.pending_leave = false;
    }
    // 分层退出：菜单 q 退出程序；子界面 q = 返回菜单（有已标记项时二次确认）；
    // 清理进行中不响应（避免中断删除）。
    if key == KeyCode::Char('q') && !matches!(app.state, AppState::Cleaning { .. }) {
        if matches!(app.state, AppState::Menu) {
            app.should_quit = true;
        } else {
            request_leave_to_menu(app, analyze_rx, tree_builder, sort_rx);
        }
        return;
    }
    match &app.state {
        AppState::Menu => handle_menu_key(app, key, events, analyze_rx, tree_builder),
        AppState::Scanning { .. } => {
            match key {
                KeyCode::Esc | KeyCode::Backspace => {
                    app.cancel_flag.store(true, Ordering::Relaxed);
                    app.state = AppState::Menu;
                    app.active_command = None;
                    app.scan_result = None;
                    app.expanded.clear();
                    app.cancel_flag = Arc::new(AtomicBool::new(false));
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.move_cursor_up();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    app.move_cursor_down();
                }
                KeyCode::Tab => {
                    // 扫描中仅允许展开/折叠浏览；标记留到扫描完成后的 Results 页，
                    // 避免扫描完成时 init_results 重新播种 marked 把用户标记冲掉（假反馈）。
                    let flat_rows = app.build_flat_rows();
                    if let Some(FlatRow::Category { cat_idx, .. }) =
                        flat_rows.get(app.result_cursor)
                    {
                        app.toggle_expand(*cat_idx);
                    }
                }
                _ => {}
            }
        }
        AppState::Results => handle_results_key(app, key, modifiers, events),
        AppState::Cleaning { .. } => {
            // 清理中不响应按键
        }
        AppState::Done { .. } => handle_done_key(app, key),
        AppState::Analyzing { .. } => handle_analyzer_key(app, key, modifiers),
        AppState::AnalyzingLive { .. } => {
            handle_analyzer_live_key(app, key, modifiers, analyze_rx, tree_builder, sort_rx);
        }
        AppState::Sorting => {
            match key {
                KeyCode::Esc | KeyCode::Backspace => {
                    app.state = AppState::Menu;
                    app.active_command = None;
                    *sort_rx = None; // drop Receiver 让排序线程 send 失败
                }
                _ => {}
            }
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
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {e:?}")));
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
                        let root_len = home.components().count();
                        let walker = mc_core::create_walker(std::path::Path::new(&home))
                            .process_read_dir(|_depth, _path, _state, children| {
                                mc_core::prefetch_metadata(children);
                            });
                        let mut count = 0u64;
                        let mut total = 0u64;
                        for entry in walker.into_iter().filter_map(std::result::Result::ok) {
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
                    Engine::scan_uninstall(&reporter);
                }));
                if let Err(e) = result {
                    reporter.on_event(ProgressEvent::Error(format!("内部错误: {e:?}")));
                }
            });
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
                // 显示相对 home 的当前扫描目录（末尾若干字符），随遍历实时移动，
                // 让用户看到"正在扫描哪里"而非静止的顶层名。渲染节流已限制刷新频率，
                // 不会狂闪；相比只显示顶层名，深层路径更能传达"在动"。
                let home = platform::get_home_dir();
                let rel = path.strip_prefix(&home).unwrap_or(path.as_path());
                let s = rel.to_string_lossy();
                let char_count = s.chars().count();
                let new_text = if char_count > 46 {
                    let tail: String = s.chars().skip(char_count - 43).collect();
                    format!("当前: …{tail}")
                } else {
                    format!("当前: ~/{s}")
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
            ..
        } => {
            // __analyze_tree__ 路径已废弃，但保留兼容处理避免数据丢失
            if category == "__analyze_tree__" {
                return;
            }
            use mc_core::models::{CategoryGroup, ScanItem, ScanResult};

            if app.scan_result.is_none() {
                app.scan_result = Some(ScanResult::default());
            }

            // Clean 流式上报同一 (category, root.path) 的增量，此处按 path 合并到既有聚合项，
            // 避免重复插入。merged=true 表示只是给既有项累加 size（不新增计数）。
            let mut merged = false;
            if let Some(result) = app.scan_result.as_mut() {
                if let Some(cat) = result.categories.iter_mut().find(|c| c.name == category) {
                    if let Some(existing) = cat.items.iter_mut().find(|it| it.path == path) {
                        existing.size += size;
                        cat.total_size += size;
                        merged = true;
                    } else {
                        cat.file_count += 1;
                        cat.total_size += size;
                        cat.items
                            .push(ScanItem::new(path, size, safety, category.clone()));
                    }
                } else {
                    result.categories.push(CategoryGroup::new(
                        category.clone(),
                        vec![ScanItem::new(path, size, safety, category.clone())],
                    ));
                    app.expanded.push(false);
                }
                result.total_size += size;
                if !merged {
                    result.file_count += 1;
                }
            }

            if let AppState::Scanning {
                ref mut found_count,
                ref mut found_size,
                ..
            } = app.state
            {
                *found_size += size;
                if !merged {
                    *found_count += 1;
                }
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
            if let Some(ret) = app.analyzer_return.take() {
                // 分析器发起的删除：剪树并原地返回，不拆树回菜单
                restore_analyzer_after_delete(app, ret, freed, count);
            } else {
                app.state = AppState::Done {
                    message: format!(
                        "清理完成！已清理 {} 个文件，释放 {}",
                        count,
                        format_size(freed, DECIMAL)
                    ),
                };
            }
        }
        ProgressEvent::Error(msg) => {
            if matches!(app.state, AppState::Scanning { .. } | AppState::Cleaning { .. }) {
                app.state = AppState::Done {
                    message: format!("错误: {msg}"),
                };
            }
        }
    }
}

// ===== Analyze 事件处理（拆分为 entry 和 finished 解决借用冲突）=====

/// 处理 `AnalyzeEvent::Entry` 和 Progress（不修改 `analyze_rx`）
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
                user_navigated,
                cursor,
                ..
            } = &mut app.state
            {
                let _ = builder.integrate_entry(tree_root, depth, name, path, size, is_file);
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
fn transition_to_sorting(
    app: &mut App,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
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

fn handle_analyze_finished(
    app: &mut App,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    transition_to_sorting(app, analyze_rx, tree_builder, sort_rx);
}

/// 取消分析扫描并干净地返回菜单（不保留部分树、不进入排序浏览器）。
fn cancel_analyze_to_menu(
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
fn request_leave_to_menu(
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

// ===== 各状态键盘处理 =====

/// 结果页键盘处理
fn handle_results_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers, _events: &EventHandler) {
    let flat_rows = app.build_flat_rows();

    match key {
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
            // 有激活的过滤词时，先清过滤；否则返回菜单
            if app.filter_query.is_empty() {
                app.back_to_menu();
            } else {
                app.filter_query.clear();
                app.clamp_result_cursor();
            }
        }
        KeyCode::Char('/') => {
            app.filter_active = true;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.move_cursor_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.move_cursor_down();
        }
        // 翻页：PageDown / Ctrl+d 下移、PageUp / Ctrl+u 上移
        KeyCode::PageDown => app.move_cursor_page_down(PAGE_STEP),
        KeyCode::PageUp => app.move_cursor_page_up(PAGE_STEP),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_page_down(PAGE_STEP);
        }
        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.move_cursor_page_up(PAGE_STEP);
        }
        // 跳转：Home / g 首行、End / G 末行
        KeyCode::Home | KeyCode::Char('g') => app.move_cursor_top(),
        KeyCode::End | KeyCode::Char('G') => app.move_cursor_bottom(),
        // 标记（统一键位：Space，d 为别名）——不移动光标
        KeyCode::Char(' ' | 'd') => {
            if let Some(row) = flat_rows.get(app.result_cursor) {
                let row = row.clone();
                app.toggle_selection(&row);
            }
        }
        // 展开/折叠（Enter 与 Tab 统一，**永不触发删除**）
        KeyCode::Tab | KeyCode::Enter => {
            if let Some(FlatRow::Category { cat_idx, .. }) = flat_rows.get(app.result_cursor) {
                app.toggle_expand(*cat_idx);
            }
        }
        KeyCode::Char('a') => {
            app.select_all_safe();
        }
        // 删除已标记（统一键位：x）
        KeyCode::Char('x') => {
            let list = app.results_delete_list();
            if !list.is_empty() {
                app.confirm_delete = Some(list);
            }
        }
        _ => {}
    }
}

/// 后台删除线程：把 (路径, 大小) 清单移入废纸篓，完成后 send `CleaningDone`。
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

        match Engine::clean(&refs, DeleteMode::Trash, &reporter) {
            Ok(_report) => {}
            Err(e) => {
                reporter.on_event(ProgressEvent::Error(e.to_string()));
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
fn start_cleaning_from_analyzer(app: &mut App, items: Vec<(PathBuf, u64)>, events: &EventHandler) {
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

/// 完成页键盘处理
fn handle_done_key(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Enter | KeyCode::Esc | KeyCode::Backspace => {
            app.back_to_menu();
        }
        _ => {}
    }
}

/// 获取 `nav_path` 指向的当前节点
fn resolve_nav_node<'a>(root: &'a DirNode, nav_path: &[usize]) -> &'a DirNode {
    ui::analyzer::resolve_node(root, nav_path)
}

/// 递归收集树中被标记路径的 (路径, 大小)；命中标记目录即整体计入、不再深入
fn collect_marked(node: &DirNode, marked: &HashSet<PathBuf>, out: &mut Vec<(PathBuf, u64)>) {
    if marked.contains(&node.path) {
        out.push((node.path.clone(), node.size));
        return;
    }
    for child in &node.children {
        collect_marked(child, marked, out);
    }
}

/// 分析器删除完成：从暂存树剪除已删路径、修正各层大小，校正导航后原地恢复 Analyzing。
fn restore_analyzer_after_delete(
    app: &mut App,
    ret: app::AnalyzerReturn,
    freed: u64,
    count: usize,
) {
    let app::AnalyzerReturn {
        tree,
        mut nav_path,
        mut cursor,
        mut cursor_stack,
        deleted,
    } = ret;
    let deleted_set: HashSet<PathBuf> = deleted.into_iter().collect();
    let mut tree = tree;
    {
        let root = Arc::make_mut(&mut tree);
        prune_paths(root, &deleted_set);
        clamp_nav_after_prune(root, &mut nav_path, &mut cursor, &mut cursor_stack);
    }
    app.state = AppState::Analyzing {
        tree_root: tree,
        nav_path,
        cursor,
        cursor_stack,
    };
    app.status_message = Some(format!(
        "已删除 {} 项，释放 {}",
        count,
        format_size(freed, DECIMAL)
    ));
}

/// 递归剪除 children 中路径命中 `deleted` 的节点，并自底向上按剩余 children 重算目录 size。
fn prune_paths(node: &mut DirNode, deleted: &HashSet<PathBuf>) {
    node.children.retain(|c| !deleted.contains(&c.path));
    for child in &mut node.children {
        if !child.is_file {
            prune_paths(child, deleted);
        }
    }
    if !node.is_file {
        node.size = node.children.iter().map(|c| c.size).sum();
    }
}

/// 剪枝后校正导航：截断指向已删/越界目录的 `nav_path`，并把 cursor 夹回当前层范围。
fn clamp_nav_after_prune(
    root: &DirNode,
    nav_path: &mut Vec<usize>,
    cursor: &mut usize,
    cursor_stack: &mut Vec<usize>,
) {
    let mut node = root;
    let mut valid = 0usize;
    for &idx in nav_path.iter() {
        match node.children.get(idx) {
            Some(c) if !c.is_file => {
                node = c;
                valid += 1;
            }
            _ => break,
        }
    }
    nav_path.truncate(valid);
    cursor_stack.truncate(valid);
    let current = resolve_nav_node(root, nav_path);
    let len = current.children.len();
    *cursor = if len == 0 { 0 } else { (*cursor).min(len - 1) };
}

/// 磁盘分析器键盘处理（Analyzing 状态，完成后的纯内存导航）
fn handle_analyzer_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    if let AppState::Analyzing {
        tree_root,
        nav_path,
        cursor,
        cursor_stack,
        ..
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        let len = current_node.children.len();
        match key {
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
                }
            // 翻页：PageDown / Ctrl+d 下移、PageUp / Ctrl+u 上移
            KeyCode::PageDown => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                }
            }
            KeyCode::PageUp => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                }
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
            }
            // 跳转：Home / g 首行、End / G 末行
            KeyCode::Home | KeyCode::Char('g') => {
                *cursor = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                if len > 0 {
                    *cursor = len - 1;
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
            // 标记（统一键位：Space，d 为别名）
            KeyCode::Char(' ' | 'd') => {
                let path = current_node.children.get(*cursor).map(|c| c.path.clone());
                if let Some(p) = path {
                    if !app.marked.remove(&p) {
                        app.marked.insert(p);
                    }
                }
            }
            KeyCode::Char('x') => {
                let mut list = Vec::new();
                collect_marked(tree_root, &app.marked, &mut list);
                if !list.is_empty() {
                    app.confirm_delete = Some(list);
                }
            }
            _ => {}
        }
    }
}

/// `AnalyzingLive` 状态键盘处理（增量构建中的可导航界面）
fn handle_analyzer_live_key(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    // 扫描进行中列表在实时按 size 重排，"当前项"随时变化——按位置标记/删除会误标
    // 此刻恰好最大的项（实测曾误标整个 ~/Library）。故扫描中禁用标记与删除，
    // 待遍历完成进入稳定的 Analyzing 态再操作。仅给出提示，不改标记集。
    // 排除 Ctrl 修饰：Ctrl+d 是翻页，不应被当作标记键拦截。
    if !modifiers.contains(KeyModifiers::CONTROL) && matches!(key, KeyCode::Char(' ' | 'd' | 'x')) {
        app.status_message =
            Some("扫描进行中不可标记/删除，完成后再操作".to_string());
        return;
    }
    // 先提取需要的字段进行操作
    if let AppState::AnalyzingLive {
        tree_root,
        nav_path,
        cursor,
        cursor_stack,
        user_navigated,
        ..
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        let len = current_node.children.len();
        match key {
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                    *user_navigated = true;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
                    *user_navigated = true;
                }
            // 翻页：PageDown / Ctrl+d 下移、PageUp / Ctrl+u 上移（均置 user_navigated）
            KeyCode::PageDown => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                    *user_navigated = true;
                }
            }
            KeyCode::PageUp => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
                *user_navigated = true;
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                    *user_navigated = true;
                }
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
                *user_navigated = true;
            }
            // 跳转：Home / g 首行、End / G 末行
            KeyCode::Home | KeyCode::Char('g') => {
                *cursor = 0;
                *user_navigated = true;
            }
            KeyCode::End | KeyCode::Char('G') => {
                if len > 0 {
                    *cursor = len - 1;
                    *user_navigated = true;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                // cursor 是显示序位置，经 order 映回底层存储索引再入 nav_path
                let order = crate::ui::analyzer::size_desc_order(&current_node.children);
                if let Some(&stored_idx) = order.get(*cursor) {
                    if current_node.children.get(stored_idx).is_some_and(|c| !c.is_file) {
                        *user_navigated = true;
                        cursor_stack.push(*cursor);
                        nav_path.push(stored_idx);
                        *cursor = 0;
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if nav_path.is_empty() {
                    cancel_analyze_to_menu(app, analyze_rx, tree_builder, sort_rx);
                } else {
                    *user_navigated = true;
                    nav_path.pop();
                    *cursor = cursor_stack.pop().unwrap_or(0);
                }
            }
            // 标记/删除（Space/d/x）已在函数入口拦截禁用（扫描中列表实时重排不安全）
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::collect_marked;
    use mc_core::models::DirNode;
    use std::collections::HashSet;
    use std::path::PathBuf;

    #[test]
    fn collect_marked_prunes_marked_dir_and_recurses_unmarked() {
        // root
        //   A (marked, dir) -> A1 (file)         期望：只收 A，不下钻 A1
        //   B (unmarked, dir) -> B1 (marked), B2 期望：下钻收 B1，不收 B2
        let mut a = DirNode::new_dir(PathBuf::from("/root/A"), "A".into());
        a.size = 500;
        a.children.push(DirNode::new_file(PathBuf::from("/root/A/A1"), "A1".into(), 500));

        let mut b = DirNode::new_dir(PathBuf::from("/root/B"), "B".into());
        b.size = 300;
        b.children.push(DirNode::new_file(PathBuf::from("/root/B/B1"), "B1".into(), 200));
        b.children.push(DirNode::new_file(PathBuf::from("/root/B/B2"), "B2".into(), 100));

        let mut root = DirNode::new_dir(PathBuf::from("/root"), "root".into());
        root.children.push(a);
        root.children.push(b);

        let mut marked = HashSet::new();
        marked.insert(PathBuf::from("/root/A"));
        marked.insert(PathBuf::from("/root/B/B1"));

        let mut out = Vec::new();
        collect_marked(&root, &marked, &mut out);

        // 只应包含 A（父目录，size 500，不重复计入 A1）与 B1（200）
        assert_eq!(out.len(), 2);
        assert!(out.contains(&(PathBuf::from("/root/A"), 500)));
        assert!(out.contains(&(PathBuf::from("/root/B/B1"), 200)));
        // A1 不应出现（A 已被剪枝，不下钻）
        assert!(!out.iter().any(|(p, _)| p == &PathBuf::from("/root/A/A1")));
    }

    #[test]
    fn prune_paths_removes_marked_child_and_recomputes_size() {
        // root: big(5M) + keep(2M)，删 big 后应只剩 keep 且 root.size 重算为 2M
        let mut big = DirNode::new_dir(PathBuf::from("/r/big"), "big".into());
        big.children.push(DirNode::new_file(PathBuf::from("/r/big/f1"), "f1".into(), 5_000_000));
        big.size = 5_000_000;
        let mut keep = DirNode::new_dir(PathBuf::from("/r/keep"), "keep".into());
        keep.children.push(DirNode::new_file(PathBuf::from("/r/keep/f2"), "f2".into(), 2_000_000));
        keep.size = 2_000_000;
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        root.children.push(big);
        root.children.push(keep);
        root.size = 7_000_000;

        let mut deleted = HashSet::new();
        deleted.insert(PathBuf::from("/r/big"));
        super::prune_paths(&mut root, &deleted);

        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name, "keep");
        assert_eq!(root.size, 2_000_000);
    }

    #[test]
    fn prune_paths_removes_nested_file_and_rolls_up_size() {
        // root -> a(dir) -> {b:100, c:300}，删 b 后 a 与 root 的 size 都应重算为 300
        let mut a = DirNode::new_dir(PathBuf::from("/r/a"), "a".into());
        a.children.push(DirNode::new_file(PathBuf::from("/r/a/b"), "b".into(), 100));
        a.children.push(DirNode::new_file(PathBuf::from("/r/a/c"), "c".into(), 300));
        a.size = 400;
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        root.children.push(a);
        root.size = 400;

        let mut deleted = HashSet::new();
        deleted.insert(PathBuf::from("/r/a/b"));
        super::prune_paths(&mut root, &deleted);

        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].size, 300);
        assert_eq!(root.size, 300);
    }

    #[test]
    fn clamp_nav_truncates_invalid_path_and_clamps_cursor() {
        // root 只有 1 个子项；nav_path 指向越界 index 5、cursor 越界 9
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        root.children.push(DirNode::new_file(PathBuf::from("/r/x"), "x".into(), 10));
        let mut nav = vec![5];
        let mut cursor = 9;
        let mut stack = vec![5];
        super::clamp_nav_after_prune(&root, &mut nav, &mut cursor, &mut stack);
        assert!(nav.is_empty());
        assert!(stack.is_empty());
        assert_eq!(cursor, 0);
    }
}
