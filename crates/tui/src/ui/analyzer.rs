use crate::app::{App, AppState};
use humansize::{format_size, DECIMAL};
use mc_core::models::DirNode;
use std::collections::HashSet;
use std::path::PathBuf;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
const LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn spinner_char(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

pub(crate) fn resolve_node<'a>(root: &'a DirNode, nav_path: &[usize]) -> &'a DirNode {
    let mut node = root;
    for &idx in nav_path {
        if let Some(child) = node.children.get(idx) {
            node = child;
        } else {
            break;
        }
    }
    node
}

fn build_breadcrumb_names(root: &DirNode, nav_path: &[usize]) -> Vec<String> {
    let mut names = vec![root.name.clone()];
    let mut node = root;
    for &idx in nav_path {
        if let Some(child) = node.children.get(idx) {
            node = child;
            names.push(node.name.clone());
        } else {
            break;
        }
    }
    names
}

/// 共享的子项列表渲染函数，供 draw() 和 draw_live() 复用
///
/// 视口优化：只为可见行构建 ListItem，从 O(n) 降到 O(visible)。
/// 滚动逻辑复刻 ratatui ListState(offset=0) 的默认行为，无用户可感知变化。
fn render_children_list(
    f: &mut Frame,
    node: &DirNode,
    cursor: usize,
    marked: &HashSet<PathBuf>,
    area: Rect,
    title: &str,
) {
    let total = node.children.len();
    if total == 0 {
        // 空列表：渲染带标题的空 block
        let empty = List::new(Vec::<ListItem>::new()).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        );
        f.render_widget(empty, area);
        return;
    }

    let parent_size = if node.size > 0 { node.size } else { 1 };
    let bar_width = (area.width as usize).saturating_sub(50).max(10);

    // Block 边框占 2 行（上下各 1）
    let visible_height = (area.height as usize).saturating_sub(2);
    if visible_height == 0 {
        return;
    }

    // 防御性 clamp
    let cursor = cursor.min(total.saturating_sub(1));

    // 复刻 ratatui ListState(offset=0) 的滚动行为：
    // cursor 在第一屏时 window_start=0，超出时 cursor 置于窗口末行
    let window_start = if cursor >= visible_height {
        cursor + 1 - visible_height
    } else {
        0
    };
    let window_end = (window_start + visible_height).min(total);

    // 仅为可见区间构建 ListItem
    let items: Vec<ListItem> = node.children[window_start..window_end]
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let abs_idx = window_start + i;
            let is_cursor = abs_idx == cursor;
            let is_marked = marked.contains(&child.path);
            let is_large = child.size >= LARGE_FILE_THRESHOLD;

            let icon = if child.is_file { "  " } else { "> " };
            let percent = if parent_size > 0 {
                (child.size as f64 / parent_size as f64 * 100.0) as u16
            } else {
                0
            };

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
                name_style = name_style
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD);
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
                Span::styled(
                    format!("{:>3}% ", percent),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(bar, bar_style),
                Span::styled(mark, Style::default().fg(Color::Red)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue)),
    );

    // 使用相对索引：cursor 在窗口切片中的位置
    let mut state = ListState::default();
    state.select(Some(cursor - window_start));
    f.render_stateful_widget(list, area, &mut state);
}

/// 已完成分析的磁盘浏览器渲染（Analyzing 状态）
pub fn draw(f: &mut Frame, app: &App) {
    let (tree_root, nav_path, cursor, marked) = match &app.state {
        AppState::Analyzing {
            tree_root,
            nav_path,
            cursor,
            marked_for_delete,
            ..
        } => (tree_root, nav_path, *cursor, marked_for_delete),
        _ => return,
    };

    let node = resolve_node(tree_root, nav_path);
    let breadcrumb_names = build_breadcrumb_names(tree_root, nav_path);

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
    render_breadcrumb(f, &breadcrumb_names, chunks[0], None);

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

    // 子项列表
    render_children_list(
        f,
        node,
        cursor,
        marked,
        chunks[2],
        " 文件列表 (按大小排序) ",
    );

    let hint = Paragraph::new(
        " ↑↓/jk 移动 | Enter/l 进入目录 | Backspace/h 返回上级 | d 标记删除 | q 返回菜单",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[3]);
}

/// AnalyzingLive 状态渲染：增量构建中的可导航界面
pub fn draw_live(f: &mut Frame, app: &App) {
    let (tree_root, nav_path, cursor, marked, file_count, total_size) = match &app.state {
        AppState::AnalyzingLive {
            tree_root,
            nav_path,
            cursor,
            marked_for_delete,
            file_count,
            total_size,
            ..
        } => (
            tree_root,
            nav_path,
            *cursor,
            marked_for_delete,
            *file_count,
            *total_size,
        ),
        _ => return,
    };

    let node = resolve_node(tree_root, nav_path);
    let breadcrumb_names = build_breadcrumb_names(tree_root, nav_path);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(f.area());

    // chunks[0]: 面包屑 + spinner + 统计
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
        / 200;
    let spinner = spinner_char(tick);
    let stats_text = if nav_path.is_empty() {
        format!(
            " {} 已发现 {} 个文件, {}",
            spinner,
            file_count,
            format_size(total_size, DECIMAL),
        )
    } else {
        format!(" {} 扫描中...", spinner)
    };
    render_breadcrumb(f, &breadcrumb_names, chunks[0], Some(&stats_text));

    // chunks[1]: 当前目录统计，标注 "(扫描中)"
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
        Span::styled("  (扫描中)", Style::default().fg(Color::Yellow)),
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

    // chunks[2]: 子项列表或空目录提示
    if node.children.is_empty() {
        let empty_hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  正在扫描此目录...",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(
            Block::default()
                .title(" 文件列表 (实时更新) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        );
        f.render_widget(empty_hint, chunks[2]);
    } else {
        render_children_list(
            f,
            node,
            cursor,
            marked,
            chunks[2],
            " 文件列表 (实时更新) ",
        );
    }

    // chunks[3]: 提示
    let hint = Paragraph::new(
        " ↑↓ 导航 | Enter 进入 | Esc 返回/取消 | d 标记删除 | 数据实时更新中...",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[3]);
}

/// 渲染面包屑导航栏
fn render_breadcrumb(
    f: &mut Frame,
    breadcrumb_names: &[String],
    area: Rect,
    extra_info: Option<&str>,
) {
    let mut breadcrumb_parts: Vec<Span> = Vec::new();
    for (i, name) in breadcrumb_names.iter().enumerate() {
        if i > 0 {
            breadcrumb_parts.push(Span::styled(
                " / ",
                Style::default().fg(Color::DarkGray),
            ));
        }
        let is_last = i == breadcrumb_names.len() - 1;
        let style = if is_last {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        breadcrumb_parts.push(Span::styled(name.clone(), style));
    }

    if let Some(info) = extra_info {
        breadcrumb_parts.push(Span::styled(
            format!("  {}", info),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let breadcrumb_line = Paragraph::new(Line::from(breadcrumb_parts)).block(
        Block::default()
            .title(" 磁盘分析 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(breadcrumb_line, area);
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
