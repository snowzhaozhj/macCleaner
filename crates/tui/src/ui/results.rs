use crate::app::App;
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

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
    let total_count = result.map_or(0, |r| r.file_count);
    let total_size = result.map_or(0, |r| r.total_size);

    let title = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!(" 扫描结果: {} 个文件, {} ", total_count, format_size(total_size, DECIMAL)),
            Style::default().fg(theme::c(Color::Cyan)),
        ),
        Span::styled("| ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format!(
                "已选: {} 项, {}",
                selected_count,
                format_size(selected_size, DECIMAL)
            ),
            Style::default().fg(theme::c(Color::Green)),
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
        .map(|(idx, row)| crate::ui::rows::flat_row_item(app, result, row, idx == app.result_cursor, true))
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" 分类列表 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::c(Color::Cyan))),
        );

    let mut state = ListState::default();
    state.select(Some(app.result_cursor));
    // 手动偏移滚动
    *state.offset_mut() = scroll_offset;
    f.render_stateful_widget(list, chunks[1], &mut state);

    // 右侧滚动条（内容超出一屏时才绘制）
    chrome::render_scrollbar(f, chunks[1], flat_rows.len(), app.result_cursor);

    // 选中摘要
    let summary = Paragraph::new(vec![Line::from(vec![
        Span::styled(
            format!(
                " 已选择 {} 项，共 {}",
                selected_count,
                format_size(selected_size, DECIMAL)
            ),
            Style::default().fg(theme::c(Color::Green)),
        ),
    ])])
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(summary, chunks[2]);

    // 底部提示：过滤输入模式显示输入框，否则显示键位提示
    if app.filter_active {
        chrome::render_footer(
            f,
            chunks[3],
            &format!(" 过滤: {}▏  (Enter 确认 | Esc 清除)", app.filter_query),
        );
    } else if app.filter_query.is_empty() {
        chrome::render_footer(f, chunks[3], &crate::keymap::footer_line(&app.state));
    } else {
        chrome::render_footer(
            f,
            chunks[3],
            &format!(" 过滤中: \"{}\"  (Esc 清除) | {}", app.filter_query, crate::keymap::footer_line(&app.state)),
        );
    }
}
