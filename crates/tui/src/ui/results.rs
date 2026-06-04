use crate::app::{App, FlatRow};
use humansize::{format_size, DECIMAL};
use mc_core::models::SafetyLevel;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

/// 安全等级对应的颜色
fn safety_color(safety: SafetyLevel) -> Color {
    match safety {
        SafetyLevel::Safe => Color::Green,
        SafetyLevel::Moderate => Color::Yellow,
        SafetyLevel::Risky => Color::Red,
    }
}

/// 安全等级标签
fn safety_label(safety: SafetyLevel) -> &'static str {
    match safety {
        SafetyLevel::Safe => "安全",
        SafetyLevel::Moderate => "中等",
        SafetyLevel::Risky => "危险",
    }
}

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    // 标题行：显示总计信息
    let (selected_count, selected_size) = app.selected_summary();
    let result = app.scan_result.as_ref();
    let total_count = result.map(|r| r.file_count).unwrap_or(0);
    let total_size = result.map(|r| r.total_size).unwrap_or(0);

    let title = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!(" 扫描结果: {} 个文件, {} ", total_count, format_size(total_size, DECIMAL)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("| ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(
                "已选: {} 个, {}",
                selected_count,
                format_size(selected_size, DECIMAL)
            ),
            Style::default().fg(Color::Green),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    // 构建扁平化行列表
    let flat_rows = app.build_flat_rows();
    let result = match app.scan_result.as_ref() {
        Some(r) => r,
        None => return,
    };

    let visible_height = chunks[1].height.saturating_sub(2) as usize; // 减去 border
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
                        format!(" ────── {} ──────", label),
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
                        style = style
                            .add_modifier(Modifier::BOLD)
                            .bg(Color::DarkGray);
                    }

                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!(" {} {} ", expand_icon, check),
                            style,
                        ),
                        Span::styled(
                            cat.name.clone(),
                            style.add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(
                                "  ({} 个文件, {}, {})",
                                cat.file_count,
                                format_size(cat.total_size, DECIMAL),
                                safety_label(dominant_safety),
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
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| item.path.display().to_string());

                    ListItem::new(Line::from(vec![
                        Span::styled(format!("     {} ", check), style),
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

    let list = List::new(items)
        .block(
            Block::default()
                .title(" 分类列表 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

    let mut state = ListState::default();
    state.select(Some(app.result_cursor));
    // 手动偏移滚动
    *state.offset_mut() = scroll_offset;
    f.render_stateful_widget(list, chunks[1], &mut state);

    // 选中摘要
    let summary = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!(
                " 已选择 {} 个项目，共 {}",
                selected_count,
                format_size(selected_size, DECIMAL)
            ),
            Style::default().fg(Color::Green),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(summary, chunks[2]);

    // 底部提示
    let hint = Paragraph::new(
        " ↑↓ 移动 | Space 选择 | Tab 展开/折叠 | a 全选安全项 | Enter 确认清理 | q 返回",
    )
    .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[3]);
}
