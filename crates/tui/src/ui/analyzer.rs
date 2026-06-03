use crate::app::{App, AppState};
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// 大文件阈值（100MB）
const LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024;

pub fn draw(f: &mut Frame, app: &App) {
    let (node, breadcrumb, cursor, marked) = match &app.state {
        AppState::Analyzing {
            node,
            breadcrumb,
            cursor,
            marked_for_delete,
        } => (node, breadcrumb, *cursor, marked_for_delete),
        _ => return,
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    // 面包屑导航
    let mut breadcrumb_parts: Vec<Span> = vec![Span::styled(
        " / ",
        Style::default().fg(Color::Cyan),
    )];
    for bc in breadcrumb {
        breadcrumb_parts.push(Span::styled(
            format!("{}", bc.name),
            Style::default().fg(Color::Cyan),
        ));
        breadcrumb_parts.push(Span::styled(
            " / ",
            Style::default().fg(Color::DarkGray),
        ));
    }
    breadcrumb_parts.push(Span::styled(
        node.name.clone(),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));

    let breadcrumb_line = Paragraph::new(Line::from(breadcrumb_parts))
        .block(
            Block::default()
                .title(" 磁盘分析 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(breadcrumb_line, chunks[0]);

    // 当前目录总大小
    let dir_info = Paragraph::new(vec![Line::from(vec![
        Span::styled("  总大小: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format_size(node.size, DECIMAL),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  |  {} 个子项", node.children.len()),
            Style::default().fg(Color::DarkGray),
        ),
        if !marked.is_empty() {
            Span::styled(
                format!("  |  已标记删除: {} 个", marked.len()),
                Style::default().fg(Color::Red),
            )
        } else {
            Span::raw("")
        },
    ])])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(dir_info, chunks[1]);

    // 子项列表（按大小降序排列，children 已排序）
    let parent_size = if node.size > 0 { node.size } else { 1 };
    let bar_width = (chunks[2].width as usize).saturating_sub(50).max(10);

    let items: Vec<ListItem> = node
        .children
        .iter()
        .enumerate()
        .map(|(idx, child)| {
            let is_cursor = idx == cursor;
            let is_marked = marked.contains(&child.path);
            let is_large = child.size >= LARGE_FILE_THRESHOLD;

            let icon = if child.is_file { "  " } else { "  " };
            let percent = if parent_size > 0 {
                (child.size as f64 / parent_size as f64 * 100.0) as u16
            } else {
                0
            };

            // 百分比条
            let filled = (bar_width as f64 * child.size as f64 / parent_size as f64) as usize;
            let bar: String = format!(
                "{}{}",
                "█".repeat(filled.min(bar_width)),
                "░".repeat(bar_width.saturating_sub(filled)),
            );

            let name_color = if is_marked {
                Color::Red
            } else if is_large {
                Color::Yellow
            } else if child.is_file {
                Color::White
            } else {
                Color::Cyan
            };

            let mut name_style = Style::default().fg(name_color);
            let mut bar_style = Style::default().fg(Color::Blue);
            if is_cursor {
                name_style = name_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                bar_style = bar_style.bg(Color::DarkGray);
            }
            if is_marked {
                name_style = name_style.add_modifier(Modifier::CROSSED_OUT);
            }

            let mark = if is_marked { " [D]" } else { "" };

            ListItem::new(Line::from(vec![
                Span::styled(icon, name_style),
                Span::styled(
                    format!("{:<24}", truncate_name(&child.name, 24)),
                    name_style,
                ),
                Span::styled(
                    format!(" {:>8} ", format_size(child.size, DECIMAL)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{:>3}% ", percent), Style::default().fg(Color::DarkGray)),
                Span::styled(bar, bar_style),
                Span::styled(mark, Style::default().fg(Color::Red)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" 文件列表 (按大小排序) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        );

    let mut state = ListState::default();
    state.select(Some(cursor));
    f.render_stateful_widget(list, chunks[2], &mut state);

    // 底部提示
    let hint = Paragraph::new(
        " ↑↓ 移动 | Enter 进入目录 | Backspace/Esc 返回上级 | d 标记删除 | q 返回菜单",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[3]);
}

fn truncate_name(name: &str, max_len: usize) -> String {
    let char_count = name.chars().count();
    if char_count <= max_len {
        name.to_string()
    } else if max_len > 3 {
        let prefix: String = name.chars().take(max_len - 3).collect();
        format!("{}...", prefix)
    } else {
        name.chars().take(max_len).collect()
    }
}
