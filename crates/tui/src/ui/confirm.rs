use crate::app::App;
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout, Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    // 先绘制结果页作为背景
    super::results::draw(f, app);

    // 居中弹出确认对话框
    let area = centered_rect(50, 40, f.area());

    // 清除背景
    f.render_widget(Clear, area);

    let (count, size) = app.selected_summary();

    let dialog = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            " 确认清理? ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  项目数量: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", count),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("  预计释放: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format_size(size, DECIMAL),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  文件将移至废纸篓",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [Enter] 确认  ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  [Esc] 取消",
                Style::default().fg(Color::Red),
            ),
        ]),
    ])
    .alignment(Alignment::Left)
    .block(
        Block::default()
            .title(" 确认 ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(dialog, area);
}

/// 创建居中的矩形区域
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
