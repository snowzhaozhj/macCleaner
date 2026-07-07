use crate::app::{App, AppState};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// 扫描列表边框标题（列表与空占位共用，避免漂移）
const SCAN_LIST_TITLE: &str = " 已发现 (扫描中...) ";

pub fn draw(f: &mut Frame, app: &App) {
    // Analyze 模式不再经过 scan::draw()，直接由 mod.rs 分发到 analyzer::draw_live()。
    //
    // 布局与 Analyze/Results 页统一——[header(3), body(Min), footer(1)]（body 再分 列表+详情，
    // 见下），扫描进度放在顶部 header（与 Analyze 一致），扫描完成切到 Results 时各区均不位移。
    let [header_area, body_area, footer_area] = chrome::three_row_layout(f.area());

    render_scan_header(f, app, header_area);

    // 与 Results 同源切分 body → (列表, 详情)：扫描态即渲染详情面板，让每项的
    // 路径/影响/恢复在扫描进行中就可见，而非等完成切 Results 才出现。current_detail()
    // 与状态解耦；Scanning 列表按发现序稳定（不逐帧按体积重排），光标停项即安全显示其详情。
    // 复用 results::split_body/render_detail 保证两页布局同源——扫描完成切 Results 时详情面板零位移。
    let (list_area, detail_area) = crate::ui::results::split_body(body_area);

    let has_results = app
        .scan_result
        .as_ref()
        .is_some_and(|r| !r.categories.is_empty());
    if has_results {
        crate::ui::rows::render_flat_list(f, app, list_area, SCAN_LIST_TITLE);
    } else {
        render_scanning_placeholder(f, list_area);
    }

    if let Some(area) = detail_area {
        crate::ui::results::render_detail(f, area, &app.current_detail());
    }

    chrome::render_footer(f, footer_area, &crate::keymap::footer_line(&app.state, footer_area.width as usize));
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

    let (progress_text, rule_current, rule_total, rule_name) = match &app.state {
        AppState::Scanning {
            progress_text,
            rule_current,
            rule_total,
            rule_name,
        } => (
            progress_text.as_str(),
            *rule_current,
            *rule_total,
            rule_name.as_str(),
        ),
        _ => ("", 0, 0, ""),
    };
    // 已发现项数/总大小直接由 scan_result 派生，不再在 Scanning 态冗余存储。
    let (found_count, found_size) = app
        .scan_result
        .as_ref()
        .map_or((0, 0), |r| (r.file_count, r.total_size));

    let spinner = chrome::spinner(app.tick);
    let left = vec![
        Span::styled(
            format!("{spinner} "),
            Style::default().fg(theme::activity()),
        ),
        Span::styled("扫描中: ", Style::default().fg(theme::ink_muted())),
        Span::styled(
            progress_text.to_string(),
            Style::default().fg(theme::ink()),
        ),
    ];

    let mut right = vec![
        Span::styled("已发现 ", Style::default().fg(theme::ink_muted())),
        Span::styled(
            format!("{found_count} 项"),
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  |  ", Style::default().fg(theme::ink_muted())),
        Span::styled(
            format_size(found_size, DECIMAL),
            Style::default()
                .fg(theme::success())
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if rule_total > 0 {
        right.push(Span::styled(
            format!("  |  [{rule_current}/{rule_total}] {rule_name}"),
            Style::default().fg(theme::activity()),
        ));
    }

    chrome::render_header(f, area, cmd_name, &left, right);
}

/// 尚无结果时的列表占位（与结果列表同样的边框/位置，切换无跳变）。
fn render_scanning_placeholder(f: &mut Frame, area: Rect) {
    let para = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  正在扫描，请稍候…",
            Style::default().fg(theme::ink_muted()),
        )),
    ])
    .block(
        Block::default()
            .title(SCAN_LIST_TITLE)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::accent())),
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
                .fg(theme::activity())
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
                Style::default().fg(theme::activity()),
            ),
            Span::styled(
                truncate_path(progress_text, (chunks[1].width as usize).saturating_sub(10)),
                Style::default().fg(theme::ink()),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" 清理进度 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::activity())),
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
