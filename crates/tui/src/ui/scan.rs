use crate::app::{App, AppState, FlatRow};
use humansize::{format_size, DECIMAL};
use mc_core::models::SafetyLevel;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spinner_char(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

fn safety_color(safety: SafetyLevel) -> Color {
    match safety {
        SafetyLevel::Safe => Color::Green,
        SafetyLevel::Moderate => Color::Yellow,
        SafetyLevel::Risky => Color::Red,
    }
}

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

    let hint = Paragraph::new(" Esc 取消 | 请等待扫描完成...")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);
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

    let hint = Paragraph::new(
        " ↑↓ 移动 | Space 选择 | Tab 展开/折叠 | a 全选安全项 | Esc 取消",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[3]);
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
                .fg(Color::Yellow)
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

    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        / 200;
    let spinner = spinner_char(tick);

    // 显示稳定的统计数字 + 顶层目录名（低频变化），不再显示快速闪烁的文件路径
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("  {spinner} "),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!(
                "已扫描 {} 个文件 | {} | {}",
                found_count,
                format_size(found_size, DECIMAL),
                progress_text,
            ),
            Style::default().fg(Color::White),
        ),
    ])];

    let mut info_spans = vec![
        Span::styled("  已发现: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{found_count} 个项目"),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("  |  大小: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_size(found_size, DECIMAL),
            Style::default().fg(Color::Green),
        ),
    ];

    if rule_total > 0 {
        info_spans.push(Span::styled(
            format!("  |  [{rule_current}/{rule_total}] {rule_name}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    lines.push(Line::from(info_spans));

    let info = Paragraph::new(lines).block(
        Block::default()
            .title(" 扫描进度 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
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
        .map(|(idx, row)| {
            let is_cursor = idx == app.result_cursor;
            match row {
                FlatRow::Separator { level } => {
                    let (label, color) = match level {
                        SafetyLevel::Safe => ("安全 (可放心删除)", Color::Green),
                        SafetyLevel::Moderate => ("中等风险 (删除后需重新下载)", Color::Yellow),
                        SafetyLevel::Risky => ("危险 (请谨慎操作)", Color::Red),
                    };
                    ListItem::new(Line::from(Span::styled(
                        format!(" ────── {label} ──────"),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )))
                }
                FlatRow::Category { cat_idx, expanded } => {
                    let cat = &result.categories[*cat_idx];
                    let dominant_safety = if cat.items.iter().all(|i| i.safety == SafetyLevel::Safe)
                    {
                        SafetyLevel::Safe
                    } else if cat.items.iter().any(|i| i.safety == SafetyLevel::Risky) {
                        SafetyLevel::Risky
                    } else {
                        SafetyLevel::Moderate
                    };

                    let expand_icon = if *expanded { "▼" } else { "▶" };
                    let selected_in_cat = cat.items.iter().filter(|i| i.selected).count();
                    let check = if selected_in_cat == cat.items.len() {
                        "[x]"
                    } else if selected_in_cat > 0 {
                        "[-]"
                    } else {
                        "[ ]"
                    };

                    let color = safety_color(dominant_safety);
                    let mut style = Style::default().fg(color);
                    if is_cursor {
                        style = style.add_modifier(Modifier::BOLD).bg(Color::DarkGray);
                    }

                    ListItem::new(Line::from(vec![
                        Span::styled(format!(" {expand_icon} {check} "), style),
                        Span::styled(cat.name.clone(), style.add_modifier(Modifier::BOLD)),
                        Span::styled(
                            format!(
                                "  ({} 个文件, {})",
                                cat.file_count,
                                format_size(cat.total_size, DECIMAL),
                            ),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                }
                FlatRow::Item { cat_idx, item_idx } => {
                    let item = &result.categories[*cat_idx].items[*item_idx];
                    let check = if item.selected { "[x]" } else { "[ ]" };
                    let color = safety_color(item.safety);

                    let mut style = Style::default().fg(color);
                    if is_cursor {
                        style = style.bg(Color::DarkGray);
                    }

                    let path_str = item
                        .path
                        .file_name().map_or_else(|| item.path.display().to_string(), |n| n.to_string_lossy().to_string());

                    ListItem::new(Line::from(vec![
                        Span::styled(format!("     {check} "), style),
                        Span::styled(path_str, style),
                        Span::styled(
                            format!("  ({})", format_size(item.size, DECIMAL)),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                }
            }
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 已发现 (扫描中...) ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
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
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    let progress_text = match &app.state {
        AppState::Cleaning { progress_text } => progress_text.as_str(),
        _ => "",
    };

    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        / 200;
    let spinner = spinner_char(tick);

    let info = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {spinner} "),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                truncate_path(progress_text, (chunks[1].width as usize).saturating_sub(10)),
                Style::default().fg(Color::White),
            ),
        ]),
    ])
    .block(
        Block::default()
            .title(" 清理进度 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(info, chunks[1]);

    let hint = Paragraph::new(" 请等待清理完成...")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);
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
