use crate::app::{App, AppState};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// 扫描列表边框标题（列表与空占位共用，避免漂移）
const SCAN_LIST_TITLE: &str = " 已发现 (扫描中...) ";

pub fn draw(f: &mut Frame, app: &App) {
    // Analyze 模式不再经过 scan::draw()，直接由 mod.rs 分发到 analyzer::draw_live()。
    //
    // 布局与 Analyze/Results 页统一——[header(3), 列表(Min), footer(1)]，扫描进度放在
    // 顶部 header（与 Analyze 一致），扫描完成切到 Results 时 header/列表/footer 均不位移。
    let [header_area, list_area, footer_area] = chrome::three_row_layout(f.area());

    render_scan_header(f, app, header_area);

    let has_results = app
        .scan_result
        .as_ref()
        .is_some_and(|r| !r.categories.is_empty());
    if has_results {
        crate::ui::rows::render_flat_list(f, app, list_area, SCAN_LIST_TITLE);
    } else {
        render_scanning_placeholder(f, list_area);
    }

    chrome::render_footer(f, footer_area, &crate::keymap::footer_line(&app.state));
}

/// 顶部 header（与 Analyze 一致）：左侧 spinner + 当前扫描路径；右侧已发现项/大小/规则进度。
fn render_scan_header(f: &mut Frame, app: &App, area: Rect) {
    let cmd_name = match app.active_command {
        Some(crate::app::ActiveCommand::Clean) => " 系统缓存扫描 ",
        Some(crate::app::ActiveCommand::Purge) => " 开发产物扫描 ",
        Some(crate::app::ActiveCommand::Uninstall) => " 应用扫描 ",
        Some(crate::app::ActiveCommand::Analyze) => " 磁盘分析 ",
        None => " 扫描 ",
    };

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
            _ => ("", 0, 0, 0, 0, ""),
        };

    let spinner = chrome::spinner(app.tick);
    let left = vec![
        Span::styled(
            format!("{spinner} "),
            Style::default().fg(theme::c(Color::Yellow)),
        ),
        Span::styled("扫描中: ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            progress_text.to_string(),
            Style::default().fg(theme::c(Color::White)),
        ),
    ];

    let mut right = vec![
        Span::styled("已发现 ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format!("{found_count} 项"),
            Style::default()
                .fg(theme::c(Color::Cyan))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format_size(found_size, DECIMAL),
            Style::default()
                .fg(theme::c(Color::Green))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if rule_total > 0 {
        right.push(Span::styled(
            format!("  |  [{rule_current}/{rule_total}] {rule_name}"),
            Style::default().fg(theme::c(Color::Yellow)),
        ));
    }

    chrome::render_header(f, area, cmd_name, left, right);
}

/// 尚无结果时的列表占位（与结果列表同样的边框/位置，切换无跳变）。
fn render_scanning_placeholder(f: &mut Frame, area: Rect) {
    let para = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  正在扫描，请稍候…",
            Style::default().fg(theme::c(Color::DarkGray)),
        )),
    ])
    .block(
        Block::default()
            .title(SCAN_LIST_TITLE)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::c(Color::Cyan))),
    );
    f.render_widget(para, area);
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
