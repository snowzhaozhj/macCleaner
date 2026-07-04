use crate::app::{App, AppState};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    // Analyze 模式不再经过 scan::draw()，直接由 mod.rs 分发到 analyzer::draw_live()

    let has_results = app
        .scan_result
        .as_ref()
        .is_some_and(|r| !r.categories.is_empty());

    if has_results {
        draw_with_results(f, app);
    } else {
        draw_scanning_only(f, app);
    }
}

fn draw_scanning_only(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_title(f, app, chunks[0]);
    render_progress(f, app, chunks[1]);

    chrome::render_footer(f, chunks[2], &crate::keymap::footer_line(&app.state));
}

fn draw_with_results(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_title(f, app, chunks[0]);
    render_progress(f, app, chunks[1]);
    render_result_list(f, app, chunks[2]);

    chrome::render_footer(f, chunks[3], &crate::keymap::footer_line(&app.state));
}

fn render_title(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let cmd_name = match app.active_command {
        Some(crate::app::ActiveCommand::Clean) => "系统缓存扫描",
        Some(crate::app::ActiveCommand::Purge) => "开发产物扫描",
        Some(crate::app::ActiveCommand::Uninstall) => "应用扫描",
        Some(crate::app::ActiveCommand::Analyze) => "磁盘分析",
        None => "扫描",
    };

    let title = Paragraph::new(format!(" {cmd_name} 中..."))
        .style(
            Style::default()
                .fg(theme::c(Color::Yellow))
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, area);
}

fn render_progress(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let (progress_text, found_count, found_size, rule_current, rule_total, rule_name) =
        match &app.state {
            AppState::Scanning {
                progress_text,
                found_count,
                found_size,
                rule_current,
                rule_total,
                rule_name,
            } => (
                progress_text.as_str(),
                *found_count,
                *found_size,
                *rule_current,
                *rule_total,
                rule_name.as_str(),
            ),
            _ => return,
        };

    let spinner = chrome::spinner(app.tick);

    // 显示稳定的统计数字 + 顶层目录名（低频变化），不再显示快速闪烁的文件路径
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("  {spinner} "),
            Style::default().fg(theme::c(Color::Yellow)),
        ),
        Span::styled(
            format!(
                "已扫描 {} 个文件 | {} | {}",
                found_count,
                format_size(found_size, DECIMAL),
                progress_text,
            ),
            Style::default().fg(theme::c(Color::White)),
        ),
    ])];

    let mut info_spans = vec![
        Span::styled("  已发现: ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format!("{found_count} 个项目"),
            Style::default().fg(theme::c(Color::Cyan)),
        ),
        Span::styled("  |  大小: ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format_size(found_size, DECIMAL),
            Style::default().fg(theme::c(Color::Green)),
        ),
    ];

    if rule_total > 0 {
        info_spans.push(Span::styled(
            format!("  |  [{rule_current}/{rule_total}] {rule_name}"),
            Style::default().fg(theme::c(Color::Yellow)),
        ));
    }

    lines.push(Line::from(info_spans));

    let info = Paragraph::new(lines).block(
        Block::default()
            .title(" 扫描进度 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::c(Color::Yellow))),
    );

    f.render_widget(info, area);
}

fn render_result_list(f: &mut Frame, app: &App, area: ratatui::layout::Rect) {
    let result = match app.scan_result.as_ref() {
        Some(r) => r,
        None => return,
    };

    let flat_rows = app.build_flat_rows();
    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll_offset = if app.result_cursor >= app.result_scroll + visible_height {
        app.result_cursor.saturating_sub(visible_height - 1)
    } else if app.result_cursor < app.result_scroll {
        app.result_cursor
    } else {
        app.result_scroll
    };

    let items: Vec<ListItem> = flat_rows
        .iter()
        .enumerate()
        .map(|(idx, row)| crate::ui::rows::flat_row_item(app, result, row, idx == app.result_cursor, false))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 已发现 (扫描中...) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::c(Color::Cyan))),
    );

    let mut state = ListState::default();
    state.select(Some(app.result_cursor));
    *state.offset_mut() = scroll_offset;
    f.render_stateful_widget(list, area, &mut state);
}

pub fn draw_cleaning(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
        ])
        .split(f.area());

    let title = Paragraph::new(" 清理中...")
        .style(
            Style::default()
                .fg(theme::c(Color::Yellow))
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let progress_text = match &app.state {
        AppState::Cleaning { progress_text } => progress_text.as_str(),
        _ => "",
    };

    let spinner = chrome::spinner(app.tick);

    let info = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {spinner} "),
                Style::default().fg(theme::c(Color::Yellow)),
            ),
            Span::styled(
                truncate_path(progress_text, (chunks[1].width as usize).saturating_sub(10)),
                Style::default().fg(theme::c(Color::White)),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" 清理进度 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::c(Color::Yellow))),
    );

    f.render_widget(info, chunks[1]);

    chrome::render_footer(f, chunks[2], " 请等待清理完成...");
}

fn truncate_path(path: &str, max_len: usize) -> String {
    let char_count = path.chars().count();
    if char_count <= max_len || max_len < 10 {
        return path.to_string();
    }
    let keep = max_len - 3;
    let suffix: String = path.chars().skip(char_count - keep).collect();
    format!("...{suffix}")
}
