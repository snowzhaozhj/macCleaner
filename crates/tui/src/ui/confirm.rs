use crate::app::{App, ConfirmItem};
use crate::theme;
use crate::ui::{chrome, text};
use humansize::{format_size, DECIMAL};
use mc_core::models::SafetyLevel;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

/// 删除确认覆盖层：由 `app.confirm_delete` 驱动，叠加在当前界面之上。
///
/// 布局为"固定头 + 可滚动清单 + 钉底操作区"三段（KTD3）：操作指引行永在最底、
/// 任何终端尺寸下都不被裁（80×24 实测事故的直接对策）。清单分两层——Risky 逐条全量
/// （置顶、红、含 impact/recovery 且 wrap，plan-007 R9 契约）；非 Risky 按分类汇总
/// + 最大 Top-3 抽样（1228 项场景"逐条前 8"是假审查）。含 Risky 时升级 type-to-confirm。
pub fn draw(f: &mut Frame, app: &App) {
    let Some(list) = &app.confirm_delete else {
        return;
    };
    let count = list.len();
    let total: u64 = list.iter().map(|i| i.size).sum();
    let has_risky = app.confirm_has_risky();

    let screen = f.area();
    if screen.width == 0 || screen.height == 0 {
        return;
    }
    // 外框 80% 宽；据此估内框可用显示宽度，供路径中段省略预算。
    let box_w = (u32::from(screen.width) * 80 / 100) as u16;
    let inner_w = (box_w.saturating_sub(2)).max(4) as usize;

    let header_lines = build_header(count, total);
    let list_lines = build_list(list, inner_w);
    let footer_lines = build_footer(app, has_risky, inner_w);

    let header_h = u16::try_from(header_lines.len()).unwrap_or(1);
    let footer_h = u16::try_from(footer_lines.len()).unwrap_or(1);
    let list_h = u16::try_from(list_lines.len()).unwrap_or(u16::MAX);
    // 期望总高 = 边框(2) + 三区；封顶到 屏高-2，保证整框在屏内（不再从底部裁掉操作行）。
    let desired = header_h
        .saturating_add(footer_h)
        .saturating_add(list_h)
        .saturating_add(2);
    let box_h = desired.min(screen.height.saturating_sub(2)).max(3);

    let border = if has_risky { theme::danger() } else { theme::warning() };
    let area = chrome::centered_rect(80, box_h, screen);
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" 确认删除 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_h),
            Constraint::Min(1),
            Constraint::Length(footer_h),
        ])
        .split(inner);
    let (header_area, body_area, footer_area) = (chunks[0], chunks[1], chunks[2]);

    f.render_widget(Paragraph::new(header_lines), header_area);

    // 清单区滚动：偏移 clamp 到 [0, len-visible]（键处理侧粗 clamp，此处精确 clamp 用于渲染）。
    let visible = body_area.height as usize;
    let max_scroll = list_lines.len().saturating_sub(visible);
    let offset = app.confirm_scroll.min(max_scroll);
    let end = (offset + visible).min(list_lines.len());
    let visible_lines: Vec<Line> = list_lines[offset..end].to_vec();
    f.render_widget(Paragraph::new(visible_lines), body_area);
    if list_lines.len() > visible && visible > 0 {
        let mut sb = ScrollbarState::new(list_lines.len()).position(offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            body_area,
            &mut sb,
        );
    }

    f.render_widget(Paragraph::new(footer_lines), footer_area);
}

/// 固定头：单行数量/大小汇总（框标题已标注"确认删除"，此处不重复标题行以省纵向空间）。
fn build_header(count: usize, total: u64) -> Vec<Line<'static>> {
    vec![Line::from(vec![
        Span::styled("  项目数量: ", Style::default().fg(theme::ink_muted())),
        Span::styled(format!("{count} 项"), Style::default().fg(theme::accent())),
        Span::styled("    预计释放: ", Style::default().fg(theme::ink_muted())),
        Span::styled(
            format_size(total, DECIMAL),
            Style::default().fg(theme::success()),
        ),
    ])]
}

/// 可滚动清单：Risky 逐条全量（含 wrap 的 impact/recovery）+ 非 Risky 分类汇总 + Top-3。
/// 每个元素恰为一"视觉行"（impact/recovery 已按显示宽度预折行），使滚动与滚动条精确。
fn build_list(list: &[ConfirmItem], inner_w: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let risky: Vec<&ConfirmItem> = list.iter().filter(|i| i.safety == SafetyLevel::Risky).collect();
    let others: Vec<&ConfirmItem> = list.iter().filter(|i| i.safety != SafetyLevel::Risky).collect();

    if !risky.is_empty() {
        let red = Style::default().fg(theme::safety_color(SafetyLevel::Risky));
        let symbol = theme::safety_symbol(SafetyLevel::Risky);
        lines.push(Line::from(Span::styled(
            format!("  {symbol} 危险项（{} 项，可能不可逆）:", risky.len()),
            red.add_modifier(Modifier::BOLD),
        )));
        for item in &risky {
            let path = text::ellipsize_path(&item.path, inner_w.saturating_sub(4));
            lines.push(Line::from(vec![
                Span::styled(format!("  {symbol} "), red),
                Span::styled(path, red.add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  ({})", format_size(item.size, DECIMAL)),
                    Style::default().fg(theme::ink_muted()),
                ),
            ]));
            push_wrapped(&mut lines, "影响", &item.impact, inner_w, red);
            push_wrapped(
                &mut lines,
                "恢复",
                &item.recovery,
                inner_w,
                Style::default().fg(theme::ink_muted()),
            );
        }
        lines.push(Line::from(""));
    }

    // 非 Risky：按 category 分组（保持首次出现顺序），每组汇总 + 最大 Top-3。
    let mut groups: Vec<(String, Vec<&ConfirmItem>)> = Vec::new();
    for item in &others {
        if let Some(g) = groups.iter_mut().find(|(k, _)| k == &item.category) {
            g.1.push(item);
        } else {
            groups.push((item.category.clone(), vec![item]));
        }
    }
    for (cat, mut items) in groups {
        let n = items.len();
        let sum: u64 = items.iter().map(|i| i.size).sum();
        let label = if cat.is_empty() { "待删项" } else { cat.as_str() };
        lines.push(Line::from(vec![
            Span::styled("  ● ", Style::default().fg(theme::danger())),
            Span::styled(
                format!("{label} — {n} 项, {}", format_size(sum, DECIMAL)),
                Style::default().fg(theme::ink()),
            ),
        ]));
        items.sort_by_key(|i| std::cmp::Reverse(i.size));
        for item in items.iter().take(3) {
            let path = text::ellipsize_path(&item.path, inner_w.saturating_sub(6));
            lines.push(Line::from(vec![
                Span::styled("    ", Style::default()),
                Span::styled(path, Style::default().fg(theme::ink())),
                Span::styled(
                    format!("  ({})", format_size(item.size, DECIMAL)),
                    Style::default().fg(theme::ink_muted()),
                ),
            ]));
        }
        if n > 3 {
            lines.push(Line::from(Span::styled(
                format!("    …… 还有 {} 项", n - 3),
                Style::default().fg(theme::ink_muted()),
            )));
        }
    }
    lines
}

/// 把 `影响:`/`恢复:` 证据句按显示宽度折成多行（首行带标签，续行缩进对齐）。空串跳过。
fn push_wrapped(lines: &mut Vec<Line<'static>>, label: &str, body: &str, inner_w: usize, style: Style) {
    if body.trim().is_empty() {
        return;
    }
    let wrap_w = inner_w.saturating_sub(10).max(4);
    for (i, seg) in text::wrap_by_width(body, wrap_w).into_iter().enumerate() {
        let prefix = if i == 0 {
            format!("      {label}: ")
        } else {
            "            ".to_string()
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{seg}"), style)));
    }
}

/// 钉底操作区：KTD2 披露 + 过滤外警示 + 废纸篓注脚 + 操作指引（永不裁）。
fn build_footer(app: &App, has_risky: bool, inner_w: usize) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    for msg in app.unmarked_child_disclosures() {
        lines.push(Line::from(Span::styled(
            format!("  {}", text::truncate_end(&msg, inner_w.saturating_sub(2))),
            Style::default().fg(theme::warning()).add_modifier(Modifier::BOLD),
        )));
    }
    let hidden = app.marked_hidden_by_filter();
    if hidden > 0 {
        lines.push(Line::from(Span::styled(
            format!("  ⚠ 其中 {hidden} 项不在当前过滤视图中，仍将一并删除"),
            Style::default().fg(theme::warning()).add_modifier(Modifier::BOLD),
        )));
    }

    lines.push(Line::from(Span::styled(
        "  文件将移至废纸篓（可恢复）",
        Style::default().fg(theme::ink_muted()),
    )));

    if has_risky {
        lines.push(Line::from(Span::styled(
            format!("  含危险项：请输入 {} 确认删除（Enter 无效）", crate::CONFIRM_TOKEN),
            Style::default().fg(theme::danger()).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("  已输入: ", Style::default().fg(theme::ink_muted())),
            Span::styled(
                format!("{}▏", app.confirm_input),
                Style::default().fg(theme::warning()),
            ),
            Span::styled("    ↑↓ 滚动   [Esc] 取消", Style::default().fg(theme::danger())),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                "  [Enter] 确认  ",
                Style::default().fg(theme::success()).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  ↑↓/jk 滚动  ", Style::default().fg(theme::ink_muted())),
            Span::styled("[Esc] 取消", Style::default().fg(theme::danger())),
        ]));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn moderate(path: &str, cat: &str, size: u64) -> ConfirmItem {
        ConfirmItem {
            path: PathBuf::from(path),
            size,
            safety: SafetyLevel::Moderate,
            category: cat.into(),
            impact: String::new(),
            recovery: String::new(),
        }
    }

    #[test]
    fn confirm_risky_section_untruncated_and_type_to_confirm() {
        let mut app = App::new();
        let mut list = vec![ConfirmItem {
            path: PathBuf::from("/x/docker_vms"),
            size: 1000,
            safety: SafetyLevel::Risky,
            category: "Docker".into(),
            impact: "全部镜像和卷丢失".into(),
            recovery: "不可恢复".into(),
        }];
        // 追加 >8 个非 Risky 项：验证 Risky 项不被截断出视野
        for i in 0..12 {
            list.push(moderate(&format!("/x/nm{i}"), "Node.js", 1));
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
        app.confirm_delete = Some(vec![moderate("/x/nm", "Node.js", 1)]);
        let text = buffer_text(&app);
        assert!(text.contains("Enter"), "无 Risky 用 Enter 确认");
        assert!(!text.contains("输入delete"), "无 Risky 不应要求 type-to-confirm");
    }

    #[test]
    fn confirm_non_risky_summarizes_by_category_with_top3() {
        // 同分类 5 项 → 汇总行 + Top-3 + "还有 2 项"（分类汇总取代逐条前 8）。
        let mut app = App::new();
        let list: Vec<ConfirmItem> = (0..5)
            .map(|i| moderate(&format!("/x/cache/f{i}"), "系统缓存", (i + 1) * 1000))
            .collect();
        app.confirm_delete = Some(list);
        let text = buffer_text(&app);
        assert!(text.contains("系统缓存—5项") || text.contains("系统缓存"), "应有分类汇总行");
        assert!(text.contains("还有2项"), "Top-3 之外应折叠计数");
    }
}
