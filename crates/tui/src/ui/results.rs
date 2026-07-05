use crate::app::{App, DetailView};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use mc_core::models::SafetyLevel;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

/// 详情面板高度（含上下边框）；列表可用高度不足时折叠隐藏。
const DETAIL_HEIGHT: u16 = 4;

pub fn draw(f: &mut Frame, app: &App) {
    // 复用共享的 [header(3), body(Min), footer(1)]，再把 body 细分为 列表 + 详情面板，
    // 避免改动被 scan/analyzer 复用的 three_row_layout（改它会连带位移那两个页面）。
    let [header_area, body_area, footer_area] = chrome::three_row_layout(f.area());
    let (list_area, detail_area) = split_body(body_area);

    // header：左侧总计，右侧已选（与 Analyze 的"总大小 | 已标记"同构）
    let (selected_count, selected_size) = app.selected_summary();
    let result = app.scan_result.as_ref();
    let total_count = result.map_or(0, |r| r.file_count);
    let total_size = result.map_or(0, |r| r.total_size);

    let left = vec![
        Span::styled(
            "扫描结果: ",
            Style::default().fg(theme::c(Color::DarkGray)),
        ),
        Span::styled(
            format!("{total_count} 个文件, {}", format_size(total_size, DECIMAL)),
            Style::default().fg(theme::c(Color::Cyan)),
        ),
    ];
    let right = vec![
        Span::styled("已选: ", Style::default().fg(theme::c(Color::DarkGray))),
        Span::styled(
            format!("{selected_count} 项, {}", format_size(selected_size, DECIMAL)),
            Style::default()
                .fg(theme::c(Color::Green))
                .add_modifier(Modifier::BOLD),
        ),
    ];
    chrome::render_header(f, header_area, " 扫描结果 ", left, right);

    crate::ui::rows::render_flat_list(f, app, list_area, " 分类列表 ");

    if let Some(area) = detail_area {
        render_detail(f, area, &app.current_detail());
    }

    // footer：过滤输入模式显示输入框，否则显示键位提示
    if app.filter_active {
        chrome::render_footer(
            f,
            footer_area,
            &format!(" 过滤: {}▏  (Enter 确认 | Esc 清除)", app.filter_query),
        );
    } else if app.filter_query.is_empty() {
        chrome::render_footer(f, footer_area, &crate::keymap::footer_line(&app.state));
    } else {
        chrome::render_footer(
            f,
            footer_area,
            &format!(
                " 过滤中: \"{}\"  (Esc 清除) | {}",
                app.filter_query,
                crate::keymap::footer_line(&app.state)
            ),
        );
    }
}

/// 把 body 区细分为 (列表, 详情面板)。列表可用高度不足时折叠详情面板（返回 None）。
fn split_body(body: Rect) -> (Rect, Option<Rect>) {
    if body.height <= DETAIL_HEIGHT + 3 {
        return (body, None);
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(DETAIL_HEIGHT)])
        .split(body);
    (chunks[0], Some(chunks[1]))
}

/// 该等级的 rubric 一句话（光标停在分区/分类行时展示）。
fn level_rubric(level: SafetyLevel) -> &'static str {
    match level {
        SafetyLevel::Safe => "删除零丢失，自动按需重建，无需操作",
        SafetyLevel::Moderate => "删除零丢失，但需手动重装/重建（重新下载或重新编译）",
        SafetyLevel::Risky => "可能丢失不可再生数据/状态；默认不勾选，删除需额外确认",
    }
}

fn safety_label(level: SafetyLevel) -> &'static str {
    match level {
        SafetyLevel::Safe => "安全",
        SafetyLevel::Moderate => "中等",
        SafetyLevel::Risky => "危险",
    }
}

/// 渲染详情面板：颜色 + 形状符号 + 文字标签三通道并存，保证 `NO_COLOR` 或色盲下仍可辨。
fn render_detail(f: &mut Frame, area: Rect, detail: &DetailView) {
    let lines: Vec<Line> = match detail {
        DetailView::Empty => vec![Line::from("")],
        DetailView::Level(level) => {
            let style = Style::default().fg(theme::c(theme::safety_color(*level)));
            vec![Line::from(vec![
                Span::styled(
                    format!("{} {}", theme::safety_symbol(*level), safety_label(*level)),
                    style.add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::raw(level_rubric(*level)),
            ])]
        }
        DetailView::Item {
            safety,
            impact,
            recovery,
        } => {
            let style = Style::default().fg(theme::c(theme::safety_color(*safety)));
            let head = Span::styled(
                format!("{} {}", theme::safety_symbol(*safety), safety_label(*safety)),
                style.add_modifier(Modifier::BOLD),
            );
            let impact_line = if impact.trim().is_empty() {
                Line::from(vec![head, Span::raw("  无恢复信息")])
            } else {
                Line::from(vec![head, Span::raw(format!("  影响: {impact}"))])
            };
            let recovery_text = if recovery.trim().is_empty() {
                "恢复: 无恢复信息".to_string()
            } else {
                format!("恢复: {recovery}")
            };
            vec![impact_line, Line::from(recovery_text)]
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::c(Color::DarkGray)))
        .title(" 详情 ");
    let para = Paragraph::new(lines).block(block).wrap(Wrap { trim: true });
    f.render_widget(para, area);
}
