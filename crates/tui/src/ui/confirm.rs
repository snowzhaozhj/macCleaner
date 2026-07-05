use crate::app::App;
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use mc_core::models::SafetyLevel;
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

/// 删除确认覆盖层：由 `app.confirm_delete` 驱动，叠加在当前界面之上。
/// Results 与 Analyzer 共用同一确认框，展示数量、预计释放与待删路径清单。
/// 含 Risky 项时：Risky 分区置顶且全量展示（不受截断），并升级为 type-to-confirm（D4/R9）。
pub fn draw(f: &mut Frame, app: &App) {
    let list = match &app.confirm_delete {
        Some(l) => l,
        None => return,
    };
    let count = list.len();
    let total: u64 = list.iter().map(|i| i.size).sum();

    const MAX_SHOWN: usize = 8;

    let risky: Vec<_> = list.iter().filter(|i| i.safety == SafetyLevel::Risky).collect();
    let others: Vec<_> = list.iter().filter(|i| i.safety != SafetyLevel::Risky).collect();
    let has_risky = !risky.is_empty();

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

    // Risky 分区置顶、红色 ✕、全量（不截断）、展示真实影响 impact 与 recovery（R9）。
    if has_risky {
        let red = Style::default().fg(theme::safety_color(SafetyLevel::Risky));
        let symbol = theme::safety_symbol(SafetyLevel::Risky);
        lines.push(Line::from(Span::styled(
            format!("  {symbol} 危险项（{} 个，可能不可逆）:", risky.len()),
            red.add_modifier(Modifier::BOLD),
        )));
        for item in &risky {
            lines.push(Line::from(vec![
                Span::styled(format!("  {symbol} "), red),
                Span::styled(item.path.display().to_string(), red.add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  ({})", format_size(item.size, DECIMAL)),
                    Style::default().fg(theme::c(Color::DarkGray)),
                ),
            ]));
            if !item.impact.trim().is_empty() {
                lines.push(Line::from(Span::styled(format!("      影响: {}", item.impact), red)));
            }
            if !item.recovery.trim().is_empty() {
                lines.push(Line::from(Span::styled(
                    format!("      恢复: {}", item.recovery),
                    Style::default().fg(theme::c(Color::DarkGray)),
                )));
            }
        }
        lines.push(Line::from(""));
    }

    // 其余（非 Risky）项：最多 MAX_SHOWN 条，让用户看清"到底删什么"
    for item in others.iter().take(MAX_SHOWN) {
        lines.push(Line::from(vec![
            Span::styled("  • ", Style::default().fg(theme::c(Color::Red))),
            Span::styled(item.path.display().to_string(), Style::default().fg(theme::c(Color::White))),
            Span::styled(
                format!("  ({})", format_size(item.size, DECIMAL)),
                Style::default().fg(theme::c(Color::DarkGray)),
            ),
        ]));
    }
    if others.len() > MAX_SHOWN {
        lines.push(Line::from(Span::styled(
            format!("  …… 还有 {} 项", others.len() - MAX_SHOWN),
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

    // 页脚：含 Risky 时用 type-to-confirm（Enter 无效，需输入 token）；否则单次 Enter 确认。
    if has_risky {
        lines.push(Line::from(Span::styled(
            format!("  含危险项：请输入 {} 确认删除（Enter 无效）", crate::CONFIRM_TOKEN),
            Style::default().fg(theme::c(Color::Red)).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("  已输入: ", Style::default().fg(theme::c(Color::DarkGray))),
            Span::styled(
                format!("{}▏", app.confirm_input),
                Style::default().fg(theme::c(Color::Yellow)),
            ),
            Span::styled("    [Esc] 取消", Style::default().fg(theme::c(Color::Red))),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                "  [Enter] 确认  ",
                Style::default().fg(theme::c(Color::Green)).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  [Esc] 取消", Style::default().fg(theme::c(Color::Red))),
        ]));
    }

    let height = u16::try_from(lines.len()).unwrap_or(0).saturating_add(2);
    let border = if has_risky { Color::Red } else { Color::Yellow };
    let area = chrome::centered_rect(64, height, f.area());
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).alignment(Alignment::Left).block(
            Block::default()
                .title(" 确认删除 ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::c(border))),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ConfirmItem;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn buffer_text(app: &App) -> String {
        let backend = TestBackend::new(90, 44);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        // 去空格归一：TestBackend 用空格填充宽字符(CJK)的次单元，会在中文词中插入空格，
        // 故对内容存在性断言前统一去掉空格。
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .replace(' ', "")
    }

    #[test]
    fn confirm_risky_section_untruncated_and_type_to_confirm() {
        let mut app = App::new();
        let mut list = vec![ConfirmItem {
            path: PathBuf::from("/x/docker_vms"),
            size: 1000,
            safety: SafetyLevel::Risky,
            impact: "全部镜像和卷丢失".into(),
            recovery: "不可恢复".into(),
        }];
        // 追加 >8 个非 Risky 项：验证 Risky 项不被 MAX_SHOWN 截断出视野
        for i in 0..12 {
            list.push(ConfirmItem {
                path: PathBuf::from(format!("/x/nm{i}")),
                size: 1,
                safety: SafetyLevel::Moderate,
                impact: String::new(),
                recovery: String::new(),
            });
        }
        app.confirm_delete = Some(list);
        let text = buffer_text(&app);
        assert!(text.contains("危险"), "应有危险分区标签");
        assert!(text.contains("docker_vms"), "Risky 项应完整可见（不被截断）");
        assert!(text.contains("全部镜像和卷丢失"), "应展示 Risky 真实后果 impact");
        assert!(text.contains("输入delete"), "含 Risky 应要求 type-to-confirm");
    }

    #[test]
    fn confirm_non_risky_uses_enter() {
        let mut app = App::new();
        app.confirm_delete = Some(vec![ConfirmItem {
            path: PathBuf::from("/x/nm"),
            size: 1,
            safety: SafetyLevel::Moderate,
            impact: String::new(),
            recovery: String::new(),
        }]);
        let text = buffer_text(&app);
        assert!(text.contains("Enter"), "无 Risky 用 Enter 确认");
        assert!(!text.contains("输入delete"), "无 Risky 不应要求 type-to-confirm");
    }
}
