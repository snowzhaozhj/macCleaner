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
use ratatui::widgets::{Block, Borders, List, ListItem};
use ratatui::Frame;

/// 渲染扁平分类/文件列表到指定区域（扫描页与结果页共用，确保切换无差异）。
/// 仅 `title`（边框标题）由调用方区分。
pub fn render_flat_list(f: &mut Frame, app: &App, area: Rect, title: &str) {
    let Some(result) = app.scan_result.as_ref() else {
        return;
    };
    let flat_rows = app.build_flat_rows();

    // 过滤无匹配：列表区显示占位行而非全空白（KTD9），避免"输错过滤词却像列表被清空"。
    if flat_rows.is_empty() && !app.filter_query.is_empty() {
        use ratatui::widgets::Paragraph;
        let placeholder = Paragraph::new(Line::from(Span::styled(
            "  无匹配项（Esc 清除过滤）",
            Style::default().fg(theme::ink_muted()),
        )))
        .block(
            Block::default()
                .title(title.to_string())
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::accent())),
        );
        f.render_widget(placeholder, area);
        return;
    }
    // 视口优化（plan 009 U4）：只为可见行构建 ListItem，从 O(total) 降到 O(visible)。
    // 对齐 analyzer 的 render_children_list（Issue #4 同款）——扫描期发现项可达上千，
    // 每帧对全部行建带样式的 ListItem 是主线程重复浪费。滚动逻辑复刻 ratatui
    // ListState(offset=0) 的默认行为：光标第一屏顶到 0、超出则置于窗口末行，无用户可感知变化。
    let total = flat_rows.len();
    // 防御性 clamp（对齐 analyzer.rs 的 render_children_list）：当前 result_cursor 的唯一收缩来源
    // 是过滤，且各 filter 编辑处都已调 clamp_result_cursor，故 total>0 时恒不越界；此处再兜一层，
    // 让未来任何"渲染前收缩 flat_rows 却漏 clamp"的路径也只滑动窗口、不显示空白。
    let cursor = app.result_cursor.min(total.saturating_sub(1));
    let visible_height = chrome::list_visible_height(area);
    // 视口起始行走 chrome::window_start 单一真源，与鼠标命中测试同源（避免公式漂移）。
    let window_start = chrome::window_start(cursor, visible_height);
    let window_end = (window_start + visible_height).min(total);

    // 仅构建可见区间的行；is_cursor 用**绝对**索引判断（cursor 是全局坐标）。
    // 越界一律走 .get() 降为跳过，防御流式重排下的 TOCTOU。
    let items: Vec<ListItem> = (window_start..window_end)
        .filter_map(|idx| {
            flat_rows
                .get(idx)
                .map(|row| flat_row_item(app, result, row, idx == cursor))
        })
        .collect();

    // 窗口内行已自带光标高亮，用普通 List（offset=0）渲染即可，不再需要 ListState 偏移。
    let list = List::new(items).block(
        Block::default()
            .title(title.to_string())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::accent())),
    );
    f.render_widget(list, area);

    chrome::render_scrollbar(f, area, total, cursor);
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
                // "项"而非"个文件"：分类项可能是目录（如 node_modules），称"文件"失真（KTD9）。
                "  ({} 项, {}, {})",
                cat.file_count,
                format_size(cat.total_size, DECIMAL),
                theme::safety_label(dominant),
            );

            // 分类头**不放安全符**：它紧挨展开符 ▶/▼ 会形成"三角撞三角"的视觉冲突，
            // 且本组等级已由上方分区标题 + 名字安全色 + detail 文字标签三处表达（冗余）。
            // 安全形状符只在「分区标题」与「文件项」出现（见符号轴解耦，theme::safety_symbol）。
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {expand_icon} {check} "), style),
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
