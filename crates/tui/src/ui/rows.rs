//! 结果/扫描列表的共享行渲染。
//!
//! Results 页与扫描进行页（scan）都把 `FlatRow`（分区/分类/文件项）渲染成同一种
//! `ListItem`——安全等级形状符号 + 颜色、复选框、展开图标、大小。收敛为单一入口
//! `render_flat_list`（含光标/滚动/滚动条）与 `flat_row_item`（单行样式），两处
//! 只以边框 `title` 相区分，其余完全一致（切换无差异）。

use crate::app::{App, FlatRow};
use crate::theme;
use crate::ui::chrome;
use humansize::{format_size, DECIMAL};
use mc_core::models::{ScanResult, SafetyLevel};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

/// 渲染扁平分类/文件列表到指定区域（扫描页与结果页共用，确保切换无差异）。
/// 仅 `title`（边框标题）由调用方区分。
pub fn render_flat_list(f: &mut Frame, app: &App, area: Rect, title: &str) {
    let Some(result) = app.scan_result.as_ref() else {
        return;
    };
    let flat_rows = app.build_flat_rows();
    // 复刻 ratatui ListState(offset=0) 的默认滚动：光标在第一屏时顶到 0，
    // 超出时置于窗口末行。每帧从 cursor 计算，无独立滚动状态。
    let visible_height = (area.height as usize).saturating_sub(2);
    let scroll_offset = if visible_height > 0 && app.result_cursor >= visible_height {
        app.result_cursor + 1 - visible_height
    } else {
        0
    };

    let items: Vec<ListItem> = flat_rows
        .iter()
        .enumerate()
        .map(|(idx, row)| flat_row_item(app, result, row, idx == app.result_cursor))
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title.to_string())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::accent())),
    );

    let mut state = ListState::default();
    state.select(Some(app.result_cursor));
    *state.offset_mut() = scroll_offset;
    f.render_stateful_widget(list, area, &mut state);

    chrome::render_scrollbar(f, area, flat_rows.len(), app.result_cursor);
}

/// 一个分类的主导安全等级：含危险则危险；全安全则安全；否则中等。
fn dominant_safety(cat: &mc_core::models::CategoryGroup) -> SafetyLevel {
    if cat.items.iter().all(|i| i.safety == SafetyLevel::Safe) {
        SafetyLevel::Safe
    } else if cat.items.iter().any(|i| i.safety == SafetyLevel::Risky) {
        SafetyLevel::Risky
    } else {
        SafetyLevel::Moderate
    }
}

/// 把单个 `FlatRow` 渲染为 `ListItem`。`is_cursor` 标记该行是否为光标行（高亮）。
pub fn flat_row_item(
    app: &App,
    result: &ScanResult,
    row: &FlatRow,
    is_cursor: bool,
) -> ListItem<'static> {
    match row {
        FlatRow::Separator { level } => {
            let label = match level {
                SafetyLevel::Safe => "安全 (可放心删除)",
                SafetyLevel::Moderate => "中等风险 (删除后需重新下载)",
                SafetyLevel::Risky => "危险 (请谨慎操作)",
            };
            ListItem::new(Line::from(Span::styled(
                format!(" {} ────── {label} ──────", theme::safety_symbol(*level)),
                Style::default()
                    .fg(theme::safety_color(*level))
                    .add_modifier(Modifier::BOLD),
            )))
        }
        FlatRow::Category { cat_idx, expanded } => {
            let cat = &result.categories[*cat_idx];
            let dominant = dominant_safety(cat);

            let expand_icon = if *expanded { "▼" } else { "▶" };
            let selected_in_cat = cat.items.iter().filter(|i| app.marked.contains(&i.path)).count();
            let check = if selected_in_cat == cat.items.len() {
                "[x]"
            } else if selected_in_cat > 0 {
                "[-]"
            } else {
                "[ ]"
            };

            let mut style = Style::default().fg(theme::safety_color(dominant));
            if is_cursor {
                style = theme::cursor_highlight(style.add_modifier(Modifier::BOLD));
            }

            let detail = format!(
                "  ({} 个文件, {}, {})",
                cat.file_count,
                format_size(cat.total_size, DECIMAL),
                theme::safety_label(dominant),
            );

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {expand_icon} {} {check} ", theme::safety_symbol(dominant)),
                    style,
                ),
                Span::styled(cat.name.clone(), style.add_modifier(Modifier::BOLD)),
                Span::styled(detail, Style::default().fg(theme::ink_muted())),
            ]))
        }
        FlatRow::Item { cat_idx, item_idx } => {
            let item = &result.categories[*cat_idx].items[*item_idx];
            let check = if app.marked.contains(&item.path) { "[x]" } else { "[ ]" };

            let mut style = Style::default().fg(theme::safety_color(item.safety));
            if is_cursor {
                style = theme::cursor_highlight(style);
            }

            let path_str = item.path.file_name().map_or_else(
                || item.path.display().to_string(),
                |n| n.to_string_lossy().to_string(),
            );

            ListItem::new(Line::from(vec![
                Span::styled(format!("     {} {check} ", theme::safety_symbol(item.safety)), style),
                Span::styled(path_str, style),
                Span::styled(
                    format!("  ({})", format_size(item.size, DECIMAL)),
                    Style::default().fg(theme::ink_muted()),
                ),
            ]))
        }
    }
}
