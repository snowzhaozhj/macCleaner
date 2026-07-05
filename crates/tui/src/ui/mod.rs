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
    use ratatui::layout::{Alignment, Rect};
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

    // 宽度自适应：min(最长行显示宽 + 4, 90% 屏宽)，取代固定 48%（窄终端下会截断内容）。
    // 高度按内容行数 + 边框。二者都不超过屏幕。
    let screen = f.area();
    let max_line = lines.iter().map(Line::width).max().unwrap_or(0);
    let desired_w = u16::try_from(max_line + 4).unwrap_or(u16::MAX);
    let cap_w = (screen.width * 9 / 10).max(10);
    let width = desired_w.min(cap_w);
    let height = u16::try_from(lines.len())
        .unwrap_or(0)
        .saturating_add(2)
        .min(screen.height);
    let x = screen.x + (screen.width.saturating_sub(width)) / 2;
    let y = screen.y + (screen.height.saturating_sub(height)) / 2;
    let area = Rect { x, y, width, height };
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

/// 完成页面：有结构化报告时展示清理明细（成功/失败 + 废纸篓双注脚 + 分类小结，KTD6）；
/// 否则（空扫描/错误）退回居中单行 message。
fn draw_done(f: &mut Frame, app: &App) {
    use humansize::{format_size, DECIMAL};
    use ratatui::layout::{Constraint, Layout, Direction, Alignment};
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph};

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    if let Some(report) = &app.done_report {
        let inner_w = (chunks[0].width as usize).saturating_sub(4).max(8);
        let has_failure = !report.failed_paths.is_empty();
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled(
            format!(
                "  ✓ 成功清理 {} 项，释放 {}",
                report.succeeded,
                format_size(report.freed, DECIMAL)
            ),
            Style::default().fg(crate::theme::success()).add_modifier(Modifier::BOLD),
        )));

        if has_failure {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  ✕ {} 项未能删除（权限/占用/SIP，已保留）:", report.failed_paths.len()),
                Style::default().fg(crate::theme::danger()).add_modifier(Modifier::BOLD),
            )));
            const MAX_FAIL_SHOWN: usize = 5;
            for p in report.failed_paths.iter().take(MAX_FAIL_SHOWN) {
                lines.push(Line::from(Span::styled(
                    format!("    {}", crate::ui::text::ellipsize_path(p, inner_w.saturating_sub(4))),
                    Style::default().fg(crate::theme::danger()),
                )));
            }
            if report.failed_paths.len() > MAX_FAIL_SHOWN {
                lines.push(Line::from(Span::styled(
                    format!("    …… 还有 {} 项失败", report.failed_paths.len() - MAX_FAIL_SHOWN),
                    Style::default().fg(crate::theme::ink_muted()),
                )));
            }
        }

        // 废纸篓双注脚（TUI 无永久删除路径，恒显示）——澄清"磁盘空间为何没变"。
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  已移入废纸篓，可从废纸篓恢复",
            Style::default().fg(crate::theme::ink_muted()),
        )));
        lines.push(Line::from(Span::styled(
            "  清空废纸篓后才真正释放磁盘空间",
            Style::default().fg(crate::theme::warning()),
        )));

        // 按分类小结
        if !report.categories.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  按分类:",
                Style::default().fg(crate::theme::ink_muted()),
            )));
            for (name, count, size) in &report.categories {
                lines.push(Line::from(vec![
                    Span::styled("    ● ", Style::default().fg(crate::theme::success())),
                    Span::styled(
                        format!("{name} — {count} 项, {}", format_size(*size, DECIMAL)),
                        Style::default().fg(crate::theme::ink()),
                    ),
                ]));
            }
        }

        let border = if has_failure { crate::theme::warning() } else { crate::theme::success() };
        let para = Paragraph::new(lines).block(
            Block::default()
                .title(" 完成 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border)),
        );
        f.render_widget(para, chunks[0]);
    } else {
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
    }

    chrome::render_footer(f, chunks[1], &crate::keymap::footer_line(&app.state, chunks[1].width as usize));
}
