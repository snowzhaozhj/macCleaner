mod analyzer_ops;
mod app;
mod command;
mod delete;
mod event;
mod keymap;
mod mouse;
mod progress;
mod reporter;
mod theme;
mod throttle;
mod tree_builder;
mod ui;

pub use delete::CONFIRM_TOKEN;

use std::collections::HashSet;

use app::{App, AppState, FlatRow};
use event::EventHandler;
use tree_builder::IncrementalTreeBuilder;

use anyhow::Result;
use crossbeam_channel::Receiver;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyCode, KeyModifiers, MouseEvent,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::layout::Rect;
use mc_core::models::DirNode;
use mc_core::progress::{AnalyzeEvent, ProgressEvent};
use std::io::{self, stdout, BufWriter};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// 翻页/Ctrl+d/Ctrl+u 一次移动的行数
pub(crate) const PAGE_STEP: usize = 10;

/// 确认清单每项估算渲染行数——滚动上界 = 项数 × 此值（粗 clamp，防按住/滚过头后空滚）。
/// 键盘与鼠标 confirm 滚动共用此常量，避免两处魔法值漂移（每 Risky 项 ≤4 视觉行、每分类 ≤5 行）。
pub(crate) const CONFIRM_ROWS_PER_ITEM: usize = 5;

// ===== 导航辅助函数 =====

// ===== 核心运行逻辑 =====

pub fn run() -> Result<()> {
    // 读取 NO_COLOR 等主题环境变量（一次性）
    theme::init();

    // 设置 panic hook：确保终端在 panic 时恢复
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        // 先关鼠标捕获再离开备用屏，与初始化逆序；不还原会残留捕获污染外层 shell。
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // 初始化终端
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    // 开启鼠标捕获：由应用自处理滚轮/点击，跨终端行为一致（否则终端把滚动翻译成方向键爆发）。
    stdout().execute(EnableMouseCapture)?;

    let backend =
        ratatui::backend::CrosstermBackend::new(BufWriter::with_capacity(8192, stdout()));
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = run_app(&mut terminal);

    // 恢复终端（与初始化逆序：先关捕获，再离开备用屏）
    disable_raw_mode()?;
    stdout().execute(DisableMouseCapture)?;
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
    Mouse(MouseEvent),
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
                let mouse_idx = sel.recv(&events.mouse_rx);
                let progress_idx = sel.recv(&events.progress_rx);
                let analyze_idx = analyze_rx.as_ref().map(|rx| sel.recv(rx));
                let sort_idx = sort_rx.as_ref().map(|rx| sel.recv(rx));

                match sel.select_timeout(Duration::from_millis(100)) {
                    Ok(oper) if oper.index() == key_idx => oper
                        .recv(&events.key_rx)
                        .map_or(SelectResult::Timeout, SelectResult::Key),
                    Ok(oper) if oper.index() == mouse_idx => oper
                        .recv(&events.mouse_rx)
                        .map_or(SelectResult::Timeout, SelectResult::Mouse),
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
                SelectResult::Mouse(m) => {
                    let size = terminal.size()?;
                    mouse::handle_mouse(&mut app, m, Rect::new(0, 0, size.width, size.height));
                    terminal.draw(|f| ui::draw(f, &app))?;
                }
                SelectResult::Progress(evt) => {
                    progress::handle_progress(&mut app, evt);
                    if throttle.as_ref().is_none_or(throttle::Throttle::can_update) {
                        app.tick = app.tick.wrapping_add(1);
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                }
                SelectResult::Analyze(evt) => match evt {
                    AnalyzeEvent::Finished => {
                        progress::handle_analyze_finished(
                            &mut app,
                            &mut tree_builder,
                            &mut analyze_rx,
                            &mut sort_rx,
                        );
                        terminal.draw(|f| ui::draw(f, &app))?;
                    }
                    other => {
                        if let Some(ref mut builder) = tree_builder {
                            progress::handle_analyze_entry(&mut app, other, builder);
                        }
                        if throttle.as_ref().is_none_or(throttle::Throttle::can_update) {
                            app.tick = app.tick.wrapping_add(1);
                            terminal.draw(|f| ui::draw(f, &app))?;
                        }
                    }
                },
                SelectResult::SortDone(result) => {
                    if let Ok(sorted_tree) = result {
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
                            // live 删除的收尾（KTD1）：本次 finalize 是为删除而来（暂存非空），
                            // 树已稳定，此刻在稳定的 Analyzing 态上执行删除，走既有剪枝恢复路径。
                            if !app.pending_analyzer_delete.is_empty() {
                                let items = std::mem::take(&mut app.pending_analyzer_delete);
                                delete::start_cleaning_from_analyzer(&mut app, items, &events);
                            }
                        }
                    } else {
                        // 排序线程 panic — 彻底清态回菜单（back_to_menu 一并清 marked/pending，
                        // 避免 live 标记漏到下个命令；树已丢失，无从安全剪枝）。
                        app.back_to_menu();
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
                recv(events.mouse_rx) -> m => {
                    if let Ok(m) = m {
                        let size = terminal.size()?;
                        mouse::handle_mouse(&mut app, m, Rect::new(0, 0, size.width, size.height));
                    }
                }
                recv(events.progress_rx) -> evt => {
                    if let Ok(evt) = evt {
                        progress::handle_progress(&mut app, evt);
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
    // 删除确认覆盖层优先：Some 时只处理确认/取消/滚动并吞键
    if app.confirm_delete.is_some() {
        // 清单滚动上界（粗 clamp，防按住翻页时 confirm_scroll 无限膨胀致后续上滚"空按"；
        // 渲染侧再按可见高度精确 clamp）。每 Risky 项 ≤4 视觉行、每分类 ≤5 行，×5 足够宽松。
        let scroll_cap = app
            .confirm_delete
            .as_ref()
            .map_or(0, Vec::len)
            .saturating_mul(CONFIRM_ROWS_PER_ITEM);
        let scroll_up = |app: &mut App, n: usize| app.confirm_scroll = app.confirm_scroll.saturating_sub(n);
        let scroll_down =
            |app: &mut App, n: usize| app.confirm_scroll = (app.confirm_scroll + n).min(scroll_cap);
        // D4：待删含 Risky（不可逆内容，如 Docker 卷/dSYM）时升级为 type-to-confirm——
        // 需输入 token 才执行，且 Enter 不绑定确认（GNOME HIG：不可逆动作不绑 Enter）。
        // 有 Risky 时 j/k 归 token 输入缓冲，仅方向键/翻页滚动（避免吞字符）。
        if app.confirm_has_risky() {
            match key {
                KeyCode::Up => scroll_up(app, 1),
                KeyCode::Down => scroll_down(app, 1),
                KeyCode::PageUp => scroll_up(app, PAGE_STEP),
                KeyCode::PageDown => scroll_down(app, PAGE_STEP),
                KeyCode::Esc => {
                    app.confirm_delete = None;
                    app.confirm_input.clear();
                    app.confirm_scroll = 0;
                }
                KeyCode::Backspace => {
                    app.confirm_input.pop();
                }
                KeyCode::Char(c) => {
                    app.confirm_input.push(c);
                    if app.confirm_input.eq_ignore_ascii_case(CONFIRM_TOKEN) {
                        delete::confirm_accept(app, events, analyze_rx, tree_builder, sort_rx);
                    }
                }
                _ => {}
            }
        } else {
            match key {
                KeyCode::Enter | KeyCode::Char('y') => {
                    delete::confirm_accept(app, events, analyze_rx, tree_builder, sort_rx);
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    app.confirm_delete = None;
                    app.confirm_input.clear();
                    app.confirm_scroll = 0;
                }
                KeyCode::Up | KeyCode::Char('k') => scroll_up(app, 1),
                KeyCode::Down | KeyCode::Char('j') => scroll_down(app, 1),
                KeyCode::PageUp => scroll_up(app, PAGE_STEP),
                KeyCode::PageDown => scroll_down(app, PAGE_STEP),
                _ => {}
            }
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
            progress::request_leave_to_menu(app, analyze_rx, tree_builder, sort_rx);
        }
        return;
    }
    match &app.state {
        AppState::Menu => command::handle_menu_key(app, key, events, analyze_rx, tree_builder),
        AppState::Scanning { .. } => {
            // 扫描态现在是可操作工作区：标记按路径（Scanning 列表为稳定插入序，安全），
            // preselect 已随 Found 增量播种；删除提交时先收尾扫描再清理（KTD1）。
            let flat_rows = app.build_flat_rows();
            match key {
                KeyCode::Esc | KeyCode::Backspace => {
                    // 与 q 走同一条返回菜单路径（取消扫描 + back_to_menu 彻底清态），
                    // 不再内联重复实现，避免两处清理逻辑漂移。
                    progress::request_leave_to_menu(app, analyze_rx, tree_builder, sort_rx);
                }
                KeyCode::Up | KeyCode::Char('k') => app.move_cursor_up(),
                KeyCode::Down | KeyCode::Char('j') => app.move_cursor_down(),
                // 展开：Tab / Enter / l（l 仅展开，不折叠，与 Results 一致，U6/KTD4）
                KeyCode::Tab | KeyCode::Enter => {
                    if let Some(FlatRow::Category { cat_idx, .. }) = flat_rows.get(app.result_cursor)
                    {
                        app.toggle_expand(*cat_idx);
                    }
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    if let Some(FlatRow::Category { cat_idx, expanded: false }) =
                        flat_rows.get(app.result_cursor)
                    {
                        app.toggle_expand(*cat_idx);
                    }
                }
                // 折叠/回分类头（h/←）：扫描态不借此返回菜单（返回仅 Esc/Backspace/q），
                // 故无可折叠时静默 no-op。
                KeyCode::Left | KeyCode::Char('h') => {
                    app.collapse_or_focus_category(&flat_rows);
                }
                // 标记（Space/d）——不移光标；a 全选安全项
                KeyCode::Char(' ' | 'd') => {
                    if let Some(row) = flat_rows.get(app.result_cursor) {
                        let row = row.clone();
                        app.toggle_selection(&row);
                    }
                }
                KeyCode::Char('a') => app.select_all_safe(),
                // 删除已标记（x）：弹确认；确认后收尾扫描再清理（见 confirm_accept 的 Scanning 分支）
                KeyCode::Char('x') => {
                    let list = app.results_delete_list();
                    if !list.is_empty() {
                        app.confirm_delete = Some(list);
                        app.confirm_scroll = 0;
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
        AppState::Analyzing { .. } => analyzer_ops::handle_analyzer_key(app, key, modifiers),
        AppState::AnalyzingLive { .. } => {
            analyzer_ops::handle_analyzer_live_key(app, key, modifiers, analyze_rx, tree_builder, sort_rx);
        }
        AppState::Sorting => {
            match key {
                KeyCode::Esc | KeyCode::Backspace => {
                    progress::request_leave_to_menu(app, analyze_rx, tree_builder, sort_rx);
                }
                _ => {}
            }
        }
    }
}

// ===== 标记辅助（鼠标与键盘共用） =====

/// 切换某路径在统一标记集中的标记态（remove-else-insert 的单一真源）。
/// 取 `&mut HashSet` 而非 `&mut App`：便于在 `&mut app.state` 借用块内（键盘 analyzer 处理）
/// 对 disjoint 字段 `app.marked` 调用而不触发整体借用冲突。
pub(crate) fn toggle_marked(marked: &mut HashSet<PathBuf>, path: PathBuf) {
    if !marked.remove(&path) {
        marked.insert(path);
    }
}

// ===== 各状态键盘处理 =====

/// 结果页键盘处理
fn handle_results_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers, _events: &EventHandler) {
    let flat_rows = app.build_flat_rows();

    match key {
        KeyCode::Esc | KeyCode::Backspace => {
            // 有激活的过滤词时，先清过滤；否则返回菜单（Esc/Backspace 专责清过滤/退出）。
            if app.filter_query.is_empty() {
                app.back_to_menu();
            } else {
                app.filter_query.clear();
                app.clamp_result_cursor();
            }
        }
        // 折叠/回分类头（h/←，KTD4）：无可折叠且无过滤时——根层——返回菜单（对齐 Analyze 根层 h）。
        // 有过滤词时不清过滤（Esc 专责），保持折叠优先。
        KeyCode::Left | KeyCode::Char('h') => {
            if !app.collapse_or_focus_category(&flat_rows) && app.filter_query.is_empty() {
                app.back_to_menu();
            }
        }
        // 展开/进入（l/→，KTD4）：l 仅展开折叠的分类（方向语义，不折叠）；文件项/已展开为 no-op。
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(FlatRow::Category { cat_idx, expanded: false }) =
                flat_rows.get(app.result_cursor)
            {
                app.toggle_expand(*cat_idx);
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
                app.confirm_scroll = 0;
            }
        }
        _ => {}
    }
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
pub(crate) fn resolve_nav_node<'a>(root: &'a DirNode, nav_path: &[usize]) -> &'a DirNode {
    ui::analyzer::resolve_node(root, nav_path)
}

#[cfg(test)]
mod tests {
    use super::{handle_key, CONFIRM_TOKEN};
    use crate::analyzer_ops::collect_marked;
    use crate::app::{App, ConfirmItem};
    use crate::event::EventHandler;
    use crossterm::event::{KeyCode, KeyModifiers};
    use mc_core::models::{DirNode, SafetyLevel};
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn risky_confirm_app() -> App {
        let mut app = App::new();
        app.confirm_delete = Some(vec![ConfirmItem {
            path: PathBuf::from("/x/docker_vms"),
            size: 1,
            safety: SafetyLevel::Risky,
            category: "Docker".into(),
            impact: "卷内数据丢失".into(),
            recovery: "不可恢复".into(),
        }]);
        app
    }

    fn press(app: &mut App, key: KeyCode) {
        let events = EventHandler::new();
        handle_key(
            app,
            key,
            KeyModifiers::empty(),
            &events,
            &mut None,
            &mut None,
            &mut None,
        );
    }

    #[test]
    fn risky_confirm_enter_does_not_delete() {
        // D4 安全关键：含 Risky 时 Enter/'y' 不得确认删除，只有输入 token 才行。
        let mut app = risky_confirm_app();
        press(&mut app, KeyCode::Enter);
        assert!(app.confirm_delete.is_some(), "Risky 下 Enter 不应确认删除");
        press(&mut app, KeyCode::Char('y'));
        assert!(app.confirm_delete.is_some(), "Risky 下 'y' 不应确认删除（应作为输入字符）");
    }

    #[test]
    fn risky_confirm_wrong_token_does_not_delete_and_esc_cancels() {
        let mut app = risky_confirm_app();
        for c in "del".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        assert!(app.confirm_delete.is_some(), "未输满 token 不应确认");
        assert_eq!(app.confirm_input, "del");
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.confirm_input, "de", "Backspace 应回删输入");
        press(&mut app, KeyCode::Esc);
        assert!(app.confirm_delete.is_none(), "Esc 应取消");
        assert_eq!(app.confirm_input, "", "取消应清空输入缓冲");
    }

    #[test]
    fn confirm_token_is_ascii_lowercase() {
        // 提示文案复用 CONFIRM_TOKEN；确保它是可直接输入的小写 ASCII。
        assert_eq!(CONFIRM_TOKEN, CONFIRM_TOKEN.to_lowercase());
        assert!(CONFIRM_TOKEN.chars().all(|c| c.is_ascii_lowercase()));
    }

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
    fn found_merges_repeated_category_path_and_counts_distinct_items() {
        use super::{App, AppState};
        use crate::progress::handle_progress;
        use mc_core::models::SafetyLevel;
        use mc_core::progress::ProgressEvent;

        let mut app = App::new();
        app.state = AppState::Scanning {
            progress_text: String::new(),
            rule_current: 0,
            rule_total: 0,
            rule_name: String::new(),
        };

        let found = |path: &str, size: u64| ProgressEvent::Found {
            category: "缓存".to_string(),
            path: PathBuf::from(path),
            size,
            safety: SafetyLevel::Safe,
            impact: String::new(),
            recovery: String::new(),
            preselect: true,
        };

        // 两次同 (category, path) 的流式增量应合并到同一项、size 累加，且不新增计数
        handle_progress(&mut app, found("/root", 10));
        handle_progress(&mut app, found("/root", 5));
        let cat = &app.scan_result.as_ref().unwrap().categories[0];
        assert_eq!(cat.items.len(), 1, "同路径增量应合并为一项");
        assert_eq!(cat.items[0].size, 15);
        assert_eq!(cat.total_size, 15);
        assert_eq!(cat.file_count, 1, "合并不新增计数");

        // 不同路径 -> 新项，计数 +1
        handle_progress(&mut app, found("/root2", 7));
        let cat = &app.scan_result.as_ref().unwrap().categories[0];
        assert_eq!(cat.items.len(), 2);
        assert_eq!(cat.file_count, 2);
        assert_eq!(cat.total_size, 22);

        // header 展示的"已发现项数/大小"直接由 scan_result 派生
        assert!(matches!(app.state, AppState::Scanning { .. }), "应仍在 Scanning 态");
        let result = app.scan_result.as_ref().unwrap();
        assert_eq!(result.file_count, 2);
        assert_eq!(result.total_size, 22);
    }

    #[test]
    fn restore_analyzer_prunes_only_succeeded_and_keeps_failed() {
        // 部分移废纸篓失败时：只剪除成功项，失败项保留在树中、size 据实重算，
        // 并提示失败数 —— 避免界面与磁盘发散（评审 P2）。
        use super::{App, AppState};
        use crate::analyzer_ops::restore_analyzer_after_delete;
        use mc_core::models::DirNode;
        use std::sync::Arc;

        let mut root = DirNode::new_dir(PathBuf::from("/root"), "root".into());
        root.size = 150;
        root.children
            .push(DirNode::new_file(PathBuf::from("/root/keep"), "keep".into(), 100));
        root.children
            .push(DirNode::new_file(PathBuf::from("/root/fail"), "fail".into(), 50));

        let ret = crate::app::AnalyzerReturn {
            tree: Arc::new(root),
            nav_path: vec![],
            cursor: 0,
            cursor_stack: vec![],
            // 两项都被请求删除
            deleted: vec![PathBuf::from("/root/keep"), PathBuf::from("/root/fail")],
        };

        let mut app = App::new();
        // 仅 keep 成功、fail 失败
        restore_analyzer_after_delete(&mut app, ret, 100, 1, &[PathBuf::from("/root/keep")]);

        let AppState::Analyzing { tree_root, .. } = &app.state else {
            panic!("应回到 Analyzing 态");
        };
        let names: Vec<&str> = tree_root.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["fail"], "成功项剪除、失败项保留");
        assert_eq!(tree_root.size, 50, "size 按剩余项重算");
        assert!(
            app.status_message.as_ref().unwrap().contains("1 项失败"),
            "应提示失败数量"
        );
    }

    #[test]
    fn found_seeds_preselect_by_safety_and_flag() {
        // KTD3：Found 首次插入时按 selected(= safety != Risky && preselect) 播种 marked，
        // 合并累加不重复播种；Risky 与 preselect=false 不播种。
        use super::{App, AppState};
        use crate::progress::handle_progress;
        use mc_core::models::SafetyLevel;
        use mc_core::progress::ProgressEvent;

        let mut app = App::new();
        app.state = AppState::Scanning {
            progress_text: String::new(),
            rule_current: 0,
            rule_total: 0,
            rule_name: String::new(),
        };
        let found = |cat: &str, path: &str, size: u64, safety, preselect| ProgressEvent::Found {
            category: cat.to_string(),
            path: PathBuf::from(path),
            size,
            safety,
            impact: String::new(),
            recovery: String::new(),
            preselect,
        };

        // Safe + preselect=true → 播种
        handle_progress(&mut app, found("缓存", "/safe", 10, SafetyLevel::Safe, true));
        // 同 (category, path) 再来一条累加 → 不重复播种（仍是 1 项）
        handle_progress(&mut app, found("缓存", "/safe", 5, SafetyLevel::Safe, true));
        // Risky → 不播种
        handle_progress(&mut app, found("Docker", "/risky", 20, SafetyLevel::Risky, true));
        // preselect=false → 不播种
        handle_progress(&mut app, found("构建", "/build", 30, SafetyLevel::Moderate, false));

        assert!(app.marked.contains(&PathBuf::from("/safe")), "Safe+preselect 应播种");
        assert!(!app.marked.contains(&PathBuf::from("/risky")), "Risky 不应播种");
        assert!(!app.marked.contains(&PathBuf::from("/build")), "preselect=false 不应播种");
        assert_eq!(app.marked.len(), 1, "只应有一项，合并累加不重复播种");
    }

    fn scanning_app_with_items() -> App {
        use super::AppState;
        use mc_core::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
        let items = vec![
            ScanItem::new(PathBuf::from("/mc_test_ne/a"), 100, SafetyLevel::Safe, "缓存".into()),
            ScanItem::new(PathBuf::from("/mc_test_ne/b"), 200, SafetyLevel::Safe, "缓存".into()),
        ];
        let cat = CategoryGroup::new("缓存".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.expanded = vec![true];
        app.state = AppState::Scanning {
            progress_text: String::new(),
            rule_current: 0,
            rule_total: 0,
            rule_name: String::new(),
        };
        // rows: [Separator, Category{0}, Item{0,0}, Item{0,1}]；光标落在首个 Item。
        app.result_cursor = 2;
        app
    }

    #[test]
    fn scanning_space_marks_without_toast() {
        // U3：扫描态 Space 直接标记（路径绑定），不再弹"扫描中不可标记"提示。
        let mut app = scanning_app_with_items();
        press(&mut app, KeyCode::Char(' '));
        assert!(app.marked.contains(&PathBuf::from("/mc_test_ne/a")), "应标记光标项");
        assert!(app.status_message.is_none(), "不应有封锁提示");
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.marked.contains(&PathBuf::from("/mc_test_ne/a")), "再按取消");
    }

    #[test]
    fn scanning_x_opens_confirm_when_marked() {
        let mut app = scanning_app_with_items();
        app.marked.insert(PathBuf::from("/mc_test_ne/a"));
        press(&mut app, KeyCode::Char('x'));
        assert!(app.confirm_delete.is_some(), "有标记按 x 应弹确认");
    }

    #[test]
    fn scanning_x_noop_when_empty() {
        let mut app = scanning_app_with_items();
        press(&mut app, KeyCode::Char('x'));
        assert!(app.confirm_delete.is_none(), "无标记按 x 不弹确认");
    }

    #[test]
    fn scanning_delete_confirm_cancels_scan_and_enters_cleaning() {
        use super::AppState;
        use std::sync::atomic::Ordering;
        let mut app = scanning_app_with_items();
        app.marked.insert(PathBuf::from("/mc_test_ne/a"));
        press(&mut app, KeyCode::Char('x')); // 弹确认（Safe，非 Risky）
        press(&mut app, KeyCode::Enter); // 确认 → confirm_accept
        assert!(app.cancel_flag.load(Ordering::Relaxed), "Scanning 删除应先置 cancel_flag 收尾扫描");
        assert!(matches!(app.state, AppState::Cleaning { .. }), "应转入 Cleaning");
    }

    fn live_app(children: Vec<DirNode>) -> App {
        use super::AppState;
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        root.children = children;
        let mut app = App::new();
        app.state = AppState::AnalyzingLive {
            tree_root: root,
            nav_path: Vec::new(),
            cursor: 0,
            cursor_stack: Vec::new(),
            file_count: 0,
            total_size: 0,
            user_navigated: false,
        };
        app
    }

    #[test]
    fn live_space_marks_by_display_order_not_storage() {
        // U4/KTD2：cursor 是显示序（体积降序），Space 应标记显示序最大项，而非 children[0]。
        let mut app = live_app(vec![
            DirNode::new_file(PathBuf::from("/r/small"), "small".into(), 10),
            DirNode::new_file(PathBuf::from("/r/big"), "big".into(), 100),
            DirNode::new_file(PathBuf::from("/r/mid"), "mid".into(), 50),
        ]);
        // cursor=0 → 显示序最大 = big（存储序 idx 1）
        press(&mut app, KeyCode::Char(' '));
        assert!(app.marked.contains(&PathBuf::from("/r/big")), "应标记显示序最大项 big");
        assert!(!app.marked.contains(&PathBuf::from("/r/small")), "不应误标 children[0]");
        // 再按取消
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.marked.contains(&PathBuf::from("/r/big")));
    }

    #[test]
    fn live_space_out_of_range_is_noop() {
        use super::AppState;
        let mut app = live_app(vec![DirNode::new_file(PathBuf::from("/r/a"), "a".into(), 1)]);
        if let AppState::AnalyzingLive { cursor, .. } = &mut app.state {
            *cursor = 99; // 越界
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(app.marked.is_empty(), "越界 cursor 标记应为 no-op，不 panic");
    }

    #[test]
    fn live_space_marks_inside_subdirectory() {
        // U4/R3：钻入子目录后仍可标记（回归修复的核心场景）。
        let mut sub = DirNode::new_dir(PathBuf::from("/r/sub"), "sub".into());
        sub.size = 100;
        sub.children.push(DirNode::new_file(PathBuf::from("/r/sub/f"), "f".into(), 100));
        let mut app = live_app(vec![sub]);
        press(&mut app, KeyCode::Char('l')); // 进入 sub
        press(&mut app, KeyCode::Char(' ')); // 标记 sub/f
        assert!(app.marked.contains(&PathBuf::from("/r/sub/f")), "子目录内应可标记");
    }

    #[test]
    fn live_x_opens_confirm_from_marked() {
        // U5：live 态 x 从 marked 收集待删并弹确认。
        let mut app = live_app(vec![DirNode::new_file(PathBuf::from("/r/big"), "big".into(), 100)]);
        app.marked.insert(PathBuf::from("/r/big"));
        press(&mut app, KeyCode::Char('x'));
        assert!(app.confirm_delete.is_some(), "有标记按 x 应弹确认");
    }

    #[test]
    fn live_x_noop_when_no_marks() {
        let mut app = live_app(vec![DirNode::new_file(PathBuf::from("/r/big"), "big".into(), 100)]);
        press(&mut app, KeyCode::Char('x'));
        assert!(app.confirm_delete.is_none(), "无标记按 x 不弹确认");
    }

    #[test]
    fn live_delete_confirm_finalizes_before_deleting() {
        // U5/KTD1：live 删除确认 → 暂存待删清单 + 转 Sorting（先 finalize），
        // 不直接进 Cleaning；实际删除待 SortDone 在稳定树上执行。
        use super::AppState;
        let mut app = live_app(vec![DirNode::new_file(PathBuf::from("/r/big"), "big".into(), 100)]);
        app.marked.insert(PathBuf::from("/r/big"));
        press(&mut app, KeyCode::Char('x')); // 弹确认（Safe，非 Risky）
        press(&mut app, KeyCode::Enter); // 确认 → confirm_accept(AnalyzingLive)
        assert!(matches!(app.state, AppState::Sorting), "应先 finalize 转 Sorting");
        assert!(!app.pending_analyzer_delete.is_empty(), "待删清单应被暂存");
    }

    #[test]
    fn live_delete_confirm_during_sorting_uses_analyzer_path() {
        // 审查条目 #1 竞态：确认框展示期间扫描自然完成进入 Sorting，此时确认删除
        // 仍应走 analyzer 暂存路径（由 SortDone 消费），不落入 Results 删除（→ Cleaning）。
        use super::AppState;
        use crate::delete::confirm_accept;
        let mut app = live_app(vec![DirNode::new_file(PathBuf::from("/r/big"), "big".into(), 100)]);
        app.marked.insert(PathBuf::from("/r/big"));
        press(&mut app, KeyCode::Char('x')); // 打开确认框（live x）
        assert!(app.confirm_delete.is_some());
        // 模拟扫描自然完成、finalize 进行中：状态已切到 Sorting，confirm_delete 仍 Some。
        app.state = AppState::Sorting;
        let events = EventHandler::new();
        confirm_accept(&mut app, &events, &mut None, &mut None, &mut None);
        assert!(matches!(app.state, AppState::Sorting), "应仍在 Sorting 等 finalize，不转 Cleaning");
        assert!(!app.pending_analyzer_delete.is_empty(), "应暂存待删走 analyzer 路径");
        assert!(app.confirm_delete.is_none(), "确认框应关闭");
    }

    #[test]
    fn found_ignored_outside_scanning_state() {
        // 防污染守卫：非扫描态（如已返回 Menu）收到残留 Found 应被忽略，
        // 不得重建 scan_result 让下一个命令看到上一个命令的检测结果。
        use super::App;
        use crate::progress::handle_progress;
        use mc_core::models::SafetyLevel;
        use mc_core::progress::ProgressEvent;

        let mut app = App::new(); // 默认 Menu 态
        handle_progress(
            &mut app,
            ProgressEvent::Found {
                category: "缓存".to_string(),
                path: PathBuf::from("/root"),
                size: 10,
                safety: SafetyLevel::Safe,
                impact: String::new(),
                recovery: String::new(),
                preselect: true,
            },
        );
        assert!(app.scan_result.is_none(), "非扫描态应忽略残留 Found");
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
        crate::analyzer_ops::prune_paths(&mut root, &deleted);

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
        crate::analyzer_ops::prune_paths(&mut root, &deleted);

        assert_eq!(root.children[0].children.len(), 1);
        assert_eq!(root.children[0].size, 300);
        assert_eq!(root.size, 300);
    }

    #[test]
    fn clamp_nav_truncates_when_target_dir_deleted_and_clamps_cursor() {
        // root -> a(dir)；nav 指向 a。删除 a 后目标路径不再存在 → nav 应清空、cursor 夹回 0。
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        root.children.push(DirNode::new_dir(PathBuf::from("/r/a"), "a".into()));
        let nav_before = vec![0];
        let targets = crate::analyzer_ops::nav_path_target_paths(&root, &nav_before);

        let mut deleted = HashSet::new();
        deleted.insert(PathBuf::from("/r/a"));
        crate::analyzer_ops::prune_paths(&mut root, &deleted);

        let mut nav = nav_before.clone();
        let mut cursor = 9;
        let mut stack = vec![0];
        crate::analyzer_ops::clamp_nav_after_prune(&root, &targets, &mut nav, &mut cursor, &mut stack);
        assert!(nav.is_empty());
        assert!(stack.is_empty());
        assert_eq!(cursor, 0);
    }

    #[test]
    fn clamp_nav_follows_target_by_path_after_earlier_sibling_removed() {
        // root -> [a(dir), b(dir), c(dir)]；nav 指向 b(index 1)。
        // 删除靠前兄弟 a 后 b 左移到 index 0；按裸索引会静默指向 c，按路径应仍锁定 b。
        let mut root = DirNode::new_dir(PathBuf::from("/r"), "r".into());
        for name in ["a", "b", "c"] {
            root.children
                .push(DirNode::new_dir(PathBuf::from(format!("/r/{name}")), name.into()));
        }
        let nav_before = vec![1]; // 指向 b
        let targets = crate::analyzer_ops::nav_path_target_paths(&root, &nav_before);

        let mut deleted = HashSet::new();
        deleted.insert(PathBuf::from("/r/a"));
        crate::analyzer_ops::prune_paths(&mut root, &deleted);

        let mut nav = nav_before.clone();
        let mut cursor = 0;
        let mut stack = vec![1];
        crate::analyzer_ops::clamp_nav_after_prune(&root, &targets, &mut nav, &mut cursor, &mut stack);
        // 索引已从 1 重映射到 0，且解析出的目标仍是 b（而非 c）。
        assert_eq!(nav, vec![0]);
        assert_eq!(super::resolve_nav_node(&root, &nav).path, PathBuf::from("/r/b"));
    }

    #[test]
    fn request_leave_to_menu_two_step_confirm_when_marked() {
        use super::{App, AppState};
        use crate::progress::request_leave_to_menu;
        let mut app = App::new();
        app.state = AppState::Results;
        app.marked.insert(PathBuf::from("/r/x"));
        // 第一次按 q：有标记项 → 仅置 pending_leave + 提示，不离开。
        request_leave_to_menu(&mut app, &mut None, &mut None, &mut None);
        assert!(app.pending_leave);
        assert!(matches!(app.state, AppState::Results));
        assert!(app.status_message.is_some());
        assert_eq!(app.marked.len(), 1);
        // 第二次按 q：真正返回菜单并清空标记/提示。
        request_leave_to_menu(&mut app, &mut None, &mut None, &mut None);
        assert!(matches!(app.state, AppState::Menu));
        assert!(app.marked.is_empty());
        assert!(!app.pending_leave);
    }

    fn results_app(expanded: bool) -> App {
        use super::AppState;
        use mc_core::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
        let items = vec![
            ScanItem::new(PathBuf::from("/r/a"), 10, SafetyLevel::Safe, "缓存".into()),
            ScanItem::new(PathBuf::from("/r/b"), 20, SafetyLevel::Safe, "缓存".into()),
        ];
        let cat = CategoryGroup::new("缓存".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.expanded = vec![expanded];
        app.state = AppState::Results;
        app
    }

    #[test]
    fn results_l_expands_collapsed_category() {
        // U6：l 展开折叠的分类（方向语义）。
        let mut app = results_app(false);
        app.result_cursor = 1; // rows: [Separator, Category]
        press(&mut app, KeyCode::Char('l'));
        assert!(app.expanded[0], "l 应展开折叠分类");
    }

    #[test]
    fn results_h_on_item_collapses_parent_not_menu() {
        use super::AppState;
        let mut app = results_app(true);
        app.result_cursor = 2; // rows: [Separator, Category, Item, Item] → 首个 Item
        press(&mut app, KeyCode::Char('h'));
        assert!(!app.expanded[0], "h 于子项应折叠父分类");
        assert!(matches!(app.state, AppState::Results), "不应返回菜单");
    }

    #[test]
    fn results_h_on_root_head_returns_to_menu() {
        use super::AppState;
        let mut app = results_app(false);
        app.result_cursor = 1; // 折叠的分类头
        press(&mut app, KeyCode::Char('h'));
        assert!(matches!(app.state, AppState::Menu), "根层无可折叠+无过滤，h 返回菜单");
    }

    #[test]
    fn results_esc_still_clears_filter() {
        // Esc 仍专责清过滤（不被 h/l 语义改动影响）。
        let mut app = results_app(true);
        app.filter_query = "a".into();
        app.result_cursor = 1;
        press(&mut app, KeyCode::Esc);
        assert!(app.filter_query.is_empty(), "Esc 应清过滤");
        assert!(matches!(app.state, super::AppState::Results), "清过滤不返回菜单");
    }

    #[test]
    fn request_leave_to_menu_immediate_when_no_marks() {
        use super::{App, AppState};
        use crate::progress::request_leave_to_menu;
        let mut app = App::new();
        app.state = AppState::Results;
        // 无标记：一次按 q 即返回菜单，不进入二次确认。
        request_leave_to_menu(&mut app, &mut None, &mut None, &mut None);
        assert!(matches!(app.state, AppState::Menu));
        assert!(!app.pending_leave);
    }

    // ===== 鼠标：命中测试 / 滚动 / 视口起始行 =====

    #[test]
    fn window_start_matches_liststate_offset_zero() {
        use crate::ui::chrome::window_start;
        // 光标在第一屏：窗口从 0 开始
        assert_eq!(window_start(0, 10), 0);
        assert_eq!(window_start(9, 10), 0);
        // 光标超出一屏：钉在窗口末行 → start = cursor+1-vh
        assert_eq!(window_start(10, 10), 1);
        assert_eq!(window_start(50, 10), 41);
        // 可见高度 0：退化为 0，不 panic
        assert_eq!(window_start(5, 0), 0);
    }

    #[test]
    fn hit_row_maps_visible_rows_and_rejects_borders() {
        use crate::mouse::hit_row;
        use ratatui::layout::Rect;
        // y=3, height=12 → 上下边框各 1 行，可见内容行 = 10（全局行 4..=13）
        let area = Rect::new(0, 3, 40, 12);
        // 首个可见行 → idx 0
        assert_eq!(hit_row(area, 5, 4, 0, 100), Some(0));
        // 末个可见行 → idx 9
        assert_eq!(hit_row(area, 5, 13, 0, 100), Some(9));
        // 上边框 / 下边框 → None
        assert_eq!(hit_row(area, 5, 3, 0, 100), None);
        assert_eq!(hit_row(area, 5, 14, 0, 100), None);
        // 水平越界（col == x+width）→ None
        assert_eq!(hit_row(area, 40, 4, 0, 100), None);
        // 空列表 → None
        assert_eq!(hit_row(area, 5, 4, 0, 0), None);
    }

    #[test]
    fn hit_row_accounts_for_scroll_offset() {
        use crate::mouse::hit_row;
        use ratatui::layout::Rect;
        let area = Rect::new(0, 3, 40, 12); // 可见 10 行
        // cursor=50 → window_start = 41；点首个可见行 → idx 41
        assert_eq!(hit_row(area, 5, 4, 50, 100), Some(41));
        // cursor=50 点末个可见行 → idx 50
        assert_eq!(hit_row(area, 5, 13, 50, 100), Some(50));
    }

    #[test]
    fn hit_row_click_below_last_item_is_none() {
        use crate::mouse::hit_row;
        use ratatui::layout::Rect;
        let area = Rect::new(0, 3, 40, 12); // 可见 10 行
        // 仅 5 项，点第 8 个可见行（visible_row=7 → idx 7 ≥ total）→ None
        assert_eq!(hit_row(area, 5, 11, 0, 5), None);
        // 点第 5 个可见行（idx 4，最后一项）→ Some(4)
        assert_eq!(hit_row(area, 5, 8, 0, 5), Some(4));
    }

    #[test]
    fn scroll_cursor_steps_and_clamps() {
        use crate::mouse::scroll_cursor;
        // 向下步进 3，不越界
        let mut c = 0;
        assert!(scroll_cursor(&mut c, 100, true));
        assert_eq!(c, 3);
        // 接近末尾时 clamp 到 len-1
        let mut c = 98;
        assert!(scroll_cursor(&mut c, 100, true));
        assert_eq!(c, 99);
        // 向上 saturating 到 0
        let mut c = 1;
        assert!(scroll_cursor(&mut c, 100, false));
        assert_eq!(c, 0);
        // 已在 0 上滚：无变化
        let mut c = 0;
        assert!(!scroll_cursor(&mut c, 100, false));
        assert_eq!(c, 0);
        // 空列表：无变化、不 panic
        let mut c = 0;
        assert!(!scroll_cursor(&mut c, 0, true));
        assert_eq!(c, 0);
    }
}
