pub mod menu;
pub mod scan;
pub mod results;
pub mod confirm;
pub mod analyzer;
pub mod chrome;
pub mod rows;
pub mod text;

use crate::app::{App, AppState};
use ratatui::Frame;

/// 根据当前状态分发渲染
pub fn draw(f: &mut Frame, app: &App) {
    match &app.state {
        AppState::Menu => menu::draw(f, app),
        AppState::Scanning { .. } => scan::draw(f, app),
        AppState::Results => results::draw(f, app),
        AppState::Cleaning { .. } => scan::draw_cleaning(f, app),
        AppState::Done { .. } => draw_done(f, app),
        AppState::Analyzing { .. } => analyzer::draw(f, app),
        AppState::AnalyzingLive { .. } => analyzer::draw_live(f, app),
        AppState::Sorting => analyzer::draw_sorting(f, app),
    }

    // 删除确认覆盖层叠加在当前界面之上（Results 与 Analyzer 共用）
    if app.confirm_delete.is_some() {
        confirm::draw(f, app);
    }

    // 帮助覆盖层叠加在任意界面之上
    if app.show_help {
        draw_help_overlay(f, app);
    }

    // 瞬时状态提示：覆盖底部一行（删除结果、扫描中禁标记、返回二次确认等），下次按键即清除
    if let Some(msg) = &app.status_message {
        draw_status_message(f, msg);
    }
}

/// 在底部一行渲染瞬时状态提示，醒目底色，覆盖该行 footer
fn draw_status_message(f: &mut Frame, msg: &str) {
    use ratatui::layout::Rect;
    use ratatui::widgets::{Clear, Paragraph};

    let area = f.area();
    if area.height == 0 {
        return;
    }
    let row = Rect {
        x: area.x,
        y: area.y + area.height - 1,
        width: area.width,
        height: 1,
    };
    f.render_widget(Clear, row);
    f.render_widget(
        Paragraph::new(format!(" {msg}")).style(crate::theme::toast_style()),
        row,
    );
}

/// 居中的帮助覆盖层，内容来自 keymap 注册表（与 footer 同源）
fn draw_help_overlay(f: &mut Frame, app: &App) {
    use ratatui::layout::Alignment;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Clear, Paragraph};

    let hints = crate::keymap::hints_for(&app.state);
    let lines: Vec<Line> = hints
        .iter()
        .map(|h| {
            Line::from(vec![
                Span::styled(
                    format!("  {:<18}", h.keys),
                    Style::default()
                        .fg(crate::theme::accent())
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(h.desc, Style::default().fg(crate::theme::ink())),
            ])
        })
        .collect();

    // 高度按内容行数 + 边框，宽度取固定值，二者都不超过屏幕
    let height = u16::try_from(lines.len()).unwrap_or(0).saturating_add(2);
    let area = chrome::centered_rect(48, height, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .block(
                Block::default()
                    .title(" 帮助 (按任意键关闭) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(crate::theme::accent())),
            ),
        area,
    );
}

/// 完成页面
fn draw_done(f: &mut Frame, app: &App) {
    use ratatui::layout::{Constraint, Layout, Direction, Alignment};
    use ratatui::style::Style;
    use ratatui::widgets::{Block, Borders, Paragraph};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    let message = match &app.state {
        AppState::Done { message } => message.as_str(),
        _ => "",
    };

    let para = Paragraph::new(message)
        .block(
            Block::default()
                .title(" 完成 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(crate::theme::success())),
        )
        .style(Style::default().fg(crate::theme::success()))
        .alignment(Alignment::Center);

    f.render_widget(para, chunks[0]);

    chrome::render_footer(f, chunks[1], &crate::keymap::footer_line(&app.state));
}
