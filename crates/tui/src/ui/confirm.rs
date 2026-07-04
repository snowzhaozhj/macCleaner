use crate::app::App;
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// 删除确认覆盖层：由 `app.confirm_delete` 驱动，叠加在当前界面之上。
/// Results 与 Analyzer 共用同一确认框，展示数量、预计释放与待删路径清单。
pub fn draw(f: &mut Frame, app: &App) {
    let list = match &app.confirm_delete {
        Some(l) => l,
        None => return,
    };
    let count = list.len();
    let total: u64 = list.iter().map(|(_, s)| s).sum();

    const MAX_SHOWN: usize = 8;

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 确认删除?",
            Style::default().fg(theme::c(Color::Yellow)).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  项目数量: ", Style::default().fg(theme::c(Color::DarkGray))),
            Span::styled(format!("{count}"), Style::default().fg(theme::c(Color::Cyan))),
            Span::styled("    预计释放: ", Style::default().fg(theme::c(Color::DarkGray))),
            Span::styled(format_size(total, DECIMAL), Style::default().fg(theme::c(Color::Green))),
        ]),
        Line::from(""),
    ];

    // 展示待删路径清单（最多 MAX_SHOWN 条），让用户看清"到底删什么"
    for (path, size) in list.iter().take(MAX_SHOWN) {
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(theme::c(Color::Red))),
            Span::styled(path.display().to_string(), Style::default().fg(theme::c(Color::White))),
            Span::styled(
                format!("  ({})", format_size(*size, DECIMAL)),
                Style::default().fg(theme::c(Color::DarkGray)),
            ),
        ]));
    }
    if count > MAX_SHOWN {
        lines.push(Line::from(Span::styled(
            format!("  …… 还有 {} 项", count - MAX_SHOWN),
            Style::default().fg(theme::c(Color::DarkGray)),
        )));
    }

    // 过滤视图外仍有已标记项将被一并删除时，显式警示（审查 F1）
    let hidden = app.marked_hidden_by_filter();
    if hidden > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  ⚠ 其中 {hidden} 项不在当前过滤视图中，仍将一并删除"),
            Style::default()
                .fg(theme::c(Color::Yellow))
                .add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  文件将移至废纸篓（可恢复）",
        Style::default().fg(theme::c(Color::DarkGray)),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "  [Enter] 确认  ",
            Style::default().fg(theme::c(Color::Green)).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  [Esc] 取消", Style::default().fg(theme::c(Color::Red))),
    ]));

    let height = u16::try_from(lines.len()).unwrap_or(0).saturating_add(2);
    let area = chrome::centered_rect(60, height, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Left).block(
            Block::default()
                .title(" 确认删除 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::c(Color::Yellow))),
        ),
        area,
    );
}
