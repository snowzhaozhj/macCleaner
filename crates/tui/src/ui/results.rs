use crate::app::App;
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &App) {
    // 布局与 Analyze/扫描页统一——[header(3), 列表(Min), footer(1)]，切换无位移。
    let [header_area, list_area, footer_area] = chrome::three_row_layout(f.area());

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
