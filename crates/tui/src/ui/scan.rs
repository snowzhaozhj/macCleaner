use crate::app::{App, AppState};
use humansize::{format_size, DECIMAL};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 获取 spinner 字符（根据 tick 计数）
fn spinner_char(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

/// 扫描进度页面
pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(3),
        ])
        .split(f.area());

    let cmd_name = match app.active_command {
        Some(crate::app::ActiveCommand::Clean) => "系统缓存扫描",
        Some(crate::app::ActiveCommand::Purge) => "开发产物扫描",
        Some(crate::app::ActiveCommand::Uninstall) => "应用扫描",
        Some(crate::app::ActiveCommand::Analyze) => "磁盘分析",
        None => "扫描",
    };

    let title = Paragraph::new(format!(" {} 中...", cmd_name))
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(title, chunks[0]);

    match &app.state {
        AppState::Scanning {
            progress_text,
            found_count,
            found_size,
        } => {
            // 使用系统时间作为 tick（避免在 App 中维护 tick 计数器）
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
                        format!("  {} ", spinner),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        truncate_path(progress_text, (chunks[1].width as usize).saturating_sub(10)),
                        Style::default().fg(Color::White),
                    ),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  已发现: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{} 个项目", found_count),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled("  |  大小: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format_size(*found_size, DECIMAL),
                        Style::default().fg(Color::Green),
                    ),
                ]),
            ])
            .block(
                Block::default()
                    .title(" 扫描进度 ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            );

            f.render_widget(info, chunks[1]);
        }
        _ => {}
    }

    let hint = Paragraph::new(" 请等待扫描完成...")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, chunks[2]);
}

/// 清理进度页面
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
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
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
                format!("  {} ", spinner),
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

/// 截断过长的路径显示
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len || max_len < 10 {
        return path.to_string();
    }
    let keep = max_len - 3;
    format!("...{}", &path[path.len() - keep..])
}
