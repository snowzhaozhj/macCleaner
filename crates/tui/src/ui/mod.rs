pub mod menu;
pub mod scan;
pub mod results;
pub mod confirm;
pub mod analyzer;

use crate::app::{App, AppState};
use ratatui::Frame;

/// 根据当前状态分发渲染
pub fn draw(f: &mut Frame, app: &App) {
    match &app.state {
        AppState::Menu => menu::draw(f, app),
        AppState::Scanning { .. } => scan::draw(f, app),
        AppState::Results => results::draw(f, app),
        AppState::Confirming => confirm::draw(f, app),
        AppState::Cleaning { .. } => scan::draw_cleaning(f, app),
        AppState::Done { .. } => draw_done(f, app),
        AppState::Analyzing { .. } => analyzer::draw(f, app),
        AppState::AnalyzingLive { .. } => analyzer::draw_live(f, app),
    }
}

/// 完成页面
fn draw_done(f: &mut Frame, app: &App) {
    use ratatui::layout::{Constraint, Layout, Direction, Alignment};
    use ratatui::style::{Color, Style};
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
                .border_style(Style::default().fg(Color::Green)),
        )
        .style(Style::default().fg(Color::Green))
        .alignment(Alignment::Center);

    f.render_widget(para, chunks[0]);

    let hint = Paragraph::new(" 按 Enter 返回菜单 | q 退出")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[1]);
}
