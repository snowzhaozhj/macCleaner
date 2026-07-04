//! 结果/扫描列表的共享行渲染。
//!
//! Results 页与扫描进行页（scan）都把 `FlatRow`（分区/分类/文件项）渲染成同一种
//! `ListItem`——安全等级形状符号 + 颜色、复选框、展开图标、大小。此前两处各存一份
//! 近乎逐行相同的构建逻辑，任何主题/标记改动都要改两遍（易漂移）。本模块收敛为
//! 单一入口 `flat_row_item`，两处按 `show_safety_label` 区分唯一差异（分类详情是否
//! 追加安全等级文字）。

use crate::app::{App, FlatRow};
use crate::theme;
use humansize::{format_size, DECIMAL};
use mc_core::models::{ScanResult, SafetyLevel};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

/// 安全等级文字标签（仅 Results 分类详情使用）
fn safety_label(safety: SafetyLevel) -> &'static str {
    match safety {
        SafetyLevel::Safe => "安全",
        SafetyLevel::Moderate => "中等",
        SafetyLevel::Risky => "危险",
    }
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

/// 把单个 `FlatRow` 渲染为 `ListItem`。
///
/// - `is_cursor`：该行是否为光标所在行（高亮）。
/// - `show_safety_label`：分类详情是否追加安全等级文字（Results=true，扫描页=false）。
pub fn flat_row_item(
    app: &App,
    result: &ScanResult,
    row: &FlatRow,
    is_cursor: bool,
    show_safety_label: bool,
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

            let detail = if show_safety_label {
                format!(
                    "  ({} 个文件, {}, {})",
                    cat.file_count,
                    format_size(cat.total_size, DECIMAL),
                    safety_label(dominant),
                )
            } else {
                format!(
                    "  ({} 个文件, {})",
                    cat.file_count,
                    format_size(cat.total_size, DECIMAL),
                )
            };

            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {expand_icon} {} {check} ", theme::safety_symbol(dominant)),
                    style,
                ),
                Span::styled(cat.name.clone(), style.add_modifier(Modifier::BOLD)),
                Span::styled(detail, Style::default().fg(theme::c(Color::DarkGray))),
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
                    Style::default().fg(theme::c(Color::DarkGray)),
                ),
            ]))
        }
    }
}
