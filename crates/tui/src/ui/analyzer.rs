use crate::app::{App, AppState};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use mc_core::models::DirNode;
use std::collections::HashSet;
use std::path::PathBuf;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;
const LARGE_FILE_THRESHOLD: u64 = 100 * 1024 * 1024;

/// 按 size 降序返回 children 的**显示顺序索引排列**（稳定排序，等大小保持插入序）。
///
/// 用于 `AnalyzingLive`：增量树以 jwalk 的发现顺序追加，实时排序若原地改动
/// `children` 会破坏 `IncrementalTreeBuilder` 的 `depth_stack` 索引与 `nav_path`。
/// 因此排序仅作用于**渲染层**——本函数返回一个不改动底层树的显示排列，
/// 光标在显示序空间中解释，落到底层树时再经此排列映回存储索引。
pub(crate) fn size_desc_order(children: &[DirNode]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..children.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(children[i].size));
    order
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

/// 共享的子项列表渲染函数，供 `draw()` 和 `draw_live()` 复用
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
    sorted: bool,
) {
    let total = node.children.len();
    if total == 0 {
        // 空列表：渲染带标题的空 block
        let empty = List::new(Vec::<ListItem>::new()).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent_explore())),
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

    // 显示排列：AnalyzingLive 按 size 降序（仅渲染层，不改底层树）；
    // Analyzing 已在 finalize 排好序，用恒等排列零开销。
    let order: Vec<usize> = if sorted {
        size_desc_order(&node.children)
    } else {
        (0..total).collect()
    };

    // 复刻 ratatui ListState(offset=0) 的滚动行为：
    // cursor 在第一屏时 window_start=0，超出时 cursor 置于窗口末行
    let window_start = if cursor >= visible_height {
        cursor + 1 - visible_height
    } else {
        0
    };
    let window_end = (window_start + visible_height).min(total);

    // 仅为可见区间构建 ListItem（经 order 映射到底层 children）
    let items: Vec<ListItem> = (window_start..window_end)
        .map(|abs_idx| {
            let child = &node.children[order[abs_idx]];
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

            // 名称色语义：待删=danger、超大文件=warning、普通文件=ink、目录=accent(可下钻)
            let name_color = if is_marked {
                theme::danger()
            } else if is_large {
                theme::warning()
            } else if child.is_file {
                theme::ink()
            } else {
                theme::accent()
            };

            let mut name_style = Style::default().fg(name_color);
            let mut bar_style = Style::default().fg(theme::accent_explore());
            if is_cursor {
                name_style = theme::cursor_highlight(name_style.add_modifier(Modifier::BOLD));
                bar_style = theme::cursor_highlight(bar_style);
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
                    Style::default().fg(theme::ink_muted()),
                ),
                Span::styled(
                    format!("{percent:>3}% "),
                    Style::default().fg(theme::ink_muted()),
                ),
                Span::styled(bar, bar_style),
                Span::styled(mark, Style::default().fg(theme::danger())),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::accent_explore())),
    );

    // 使用相对索引：cursor 在窗口切片中的位置
    let mut state = ListState::default();
    state.select(Some(cursor - window_start));
    f.render_stateful_widget(list, area, &mut state);

    // 右侧滚动条（内容超出一屏时才绘制）
    chrome::render_scrollbar(f, area, total, cursor);
}

/// 已完成分析的磁盘浏览器渲染（Analyzing 状态）
pub fn draw(f: &mut Frame, app: &App) {
    let (tree_root, nav_path, cursor) = match &app.state {
        AppState::Analyzing {
            tree_root,
            nav_path,
            cursor,
            ..
        } => (tree_root, nav_path, *cursor),
        _ => return,
    };
    let marked = &app.marked;

    let node = resolve_node(tree_root, nav_path);
    let breadcrumb_names = build_breadcrumb_names(tree_root, nav_path);

    // 3 段布局：header(3) / 列表(Min) / footer(1)
    let [header_area, list_area, footer_area] = chrome::three_row_layout(f.area());

    // header 左侧：面包屑；右侧：总大小 | 子项 | 已标记
    let left = build_breadcrumb_spans(&breadcrumb_names);
    let mut right = vec![
        Span::styled("总大小: ", Style::default().fg(theme::ink_muted())),
        Span::styled(
            format_size(node.size, DECIMAL),
            Style::default()
                .fg(theme::success())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  |  {} 个子项", node.children.len()),
            Style::default().fg(theme::ink_muted()),
        ),
    ];
    if !marked.is_empty() {
        right.push(Span::styled(
            format!("  |  已标记删除: {} 个", marked.len()),
            Style::default().fg(theme::danger()),
        ));
    }
    chrome::render_header(f, header_area, " 磁盘分析 ", left, right);

    render_children_list(
        f,
        node,
        cursor,
        marked,
        list_area,
        " 文件列表 (按大小排序) ",
        false,
    );

    chrome::render_footer(f, footer_area, &crate::keymap::footer_line(&app.state));
}

/// `AnalyzingLive` 状态渲染：增量构建中的可导航界面
pub fn draw_live(f: &mut Frame, app: &App) {
    let (tree_root, nav_path, cursor, file_count, total_size) = match &app.state {
        AppState::AnalyzingLive {
            tree_root,
            nav_path,
            cursor,
            file_count,
            total_size,
            ..
        } => (tree_root, nav_path, *cursor, *file_count, *total_size),
        _ => return,
    };
    let marked = &app.marked;

    let node = resolve_node(tree_root, nav_path);
    let breadcrumb_names = build_breadcrumb_names(tree_root, nav_path);

    // 3 段布局：header(3) / 列表(Min) / footer(1)
    let [header_area, list_area, footer_area] = chrome::three_row_layout(f.area());

    // header 左侧：面包屑
    let left = build_breadcrumb_spans(&breadcrumb_names);

    // header 右侧：spinner+扫描统计 | 总大小 | 子项 | (扫描中) | 已标记
    let spinner = chrome::spinner(app.tick);
    let mut right: Vec<Span> = vec![Span::styled(
        format!("{spinner} "),
        Style::default().fg(theme::activity()),
    )];
    if nav_path.is_empty() {
        right.push(Span::styled(
            format!(
                "已发现 {} 个文件, {}  |  ",
                file_count,
                format_size(total_size, DECIMAL),
            ),
            Style::default()
                .fg(theme::activity())
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        right.push(Span::styled(
            "扫描中...  |  ",
            Style::default()
                .fg(theme::activity())
                .add_modifier(Modifier::BOLD),
        ));
    }
    right.push(Span::styled(
        "总大小: ",
        Style::default().fg(theme::ink_muted()),
    ));
    right.push(Span::styled(
        format_size(node.size, DECIMAL),
        Style::default()
            .fg(theme::success())
            .add_modifier(Modifier::BOLD),
    ));
    right.push(Span::styled(
        format!("  |  {} 个子项", node.children.len()),
        Style::default().fg(theme::ink_muted()),
    ));
    right.push(Span::styled("  (扫描中)", Style::default().fg(theme::activity())));
    if !marked.is_empty() {
        right.push(Span::styled(
            format!("  |  已标记删除: {} 个", marked.len()),
            Style::default().fg(theme::danger()),
        ));
    }
    chrome::render_header(f, header_area, " 磁盘分析 ", left, right);

    // 子项列表或空目录提示
    if node.children.is_empty() {
        let empty_hint = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  正在扫描此目录...",
                Style::default().fg(theme::ink_muted()),
            )),
        ])
        .block(
            Block::default()
                .title(" 文件列表 (实时更新) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent_explore())),
        );
        f.render_widget(empty_hint, list_area);
    } else {
        render_children_list(
            f,
            node,
            cursor,
            marked,
            list_area,
            " 文件列表 (实时更新, 按大小排序) ",
            true,
        );
    }

    chrome::render_footer(f, footer_area, &crate::keymap::footer_line(&app.state));
}

/// Sorting 过渡状态渲染：居中显示 spinner + "正在排序..."
pub fn draw_sorting(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    let spinner = chrome::spinner(app.tick);

    let text = format!("{spinner} 正在排序...");
    let para = Paragraph::new(text)
        .block(
            Block::default()
                .title(" 磁盘分析 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent())),
        )
        .style(
            Style::default()
                .fg(theme::activity())
                .add_modifier(Modifier::BOLD),
        )
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(para, chunks[0]);

    chrome::render_footer(f, chunks[1], &crate::keymap::footer_line(&app.state));
}

/// 构建面包屑导航 span 列表（供 header 左侧使用）
fn build_breadcrumb_spans(breadcrumb_names: &[String]) -> Vec<Span<'static>> {
    let mut breadcrumb_parts: Vec<Span> = Vec::new();
    for (i, name) in breadcrumb_names.iter().enumerate() {
        if i > 0 {
            breadcrumb_parts.push(Span::styled(
                " / ",
                Style::default().fg(theme::ink_muted()),
            ));
        }
        let is_last = i == breadcrumb_names.len() - 1;
        let style = if is_last {
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::accent())
        };
        breadcrumb_parts.push(Span::styled(name.clone(), style));
    }
    breadcrumb_parts
}

fn truncate_name(name: &str, max_len: usize) -> String {
    let char_count = name.chars().count();
    if char_count <= max_len {
        name.to_string()
    } else if max_len > 3 {
        let prefix: String = name.chars().take(max_len - 3).collect();
        format!("{prefix}...")
    } else {
        name.chars().take(max_len).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::size_desc_order;
    use mc_core::models::DirNode;
    use std::path::PathBuf;

    fn file(name: &str, size: u64) -> DirNode {
        DirNode::new_file(PathBuf::from(name), name.to_string(), size)
    }

    #[test]
    fn size_desc_order_sorts_descending_and_is_stable() {
        // 大小 [10, 30, 20, 30]：等大小的 idx1 与 idx3 应保持插入序（稳定）
        let children = vec![
            file("a", 10),
            file("b", 30),
            file("c", 20),
            file("d", 30),
        ];
        let order = size_desc_order(&children);
        // 期望显示序：b(30@1), d(30@3), c(20@2), a(10@0)
        assert_eq!(order, vec![1, 3, 2, 0]);
    }

    #[test]
    fn size_desc_order_is_a_valid_permutation() {
        let children = vec![file("a", 5), file("b", 1), file("c", 9), file("d", 1)];
        let mut sorted = size_desc_order(&children);
        sorted.sort_unstable();
        assert_eq!(sorted, vec![0, 1, 2, 3]); // 恰好是 0..n 的排列
    }

    #[test]
    fn size_desc_order_empty() {
        assert!(size_desc_order(&[]).is_empty());
    }
}
