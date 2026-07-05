//! 共享 Chrome 组件层。
//!
//! 消除各页重复的边框/标题/hint/spinner：
//! - `spinner` 为唯一的 spinner tick 实现，替代 scan.rs / analyzer.rs 各自的副本；
//! - `render_header` 用单个高度为 3 的边框盒承载"左侧标题/面包屑 + 右侧统计"，
//!   取代原先"面包屑盒 + 目录信息盒"两个盒子，回收竖向空间；
//! - `render_footer` 渲染单行底部提示（DarkGray，无边框）；
//! - `three_row_layout` 返回 [header, body, footer] 三段布局供各页复用。

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

/// spinner 动画帧（唯一实现，替代各页副本）
pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 根据全局 tick 返回当前 spinner 字符
#[must_use]
pub fn spinner(tick: u64) -> &'static str {
    SPINNER_FRAMES[(tick as usize) % SPINNER_FRAMES.len()]
}

/// 单行 header：`title` 边框标题（Cyan），左侧面包屑，右侧统计。
///
/// 用一个高度为 3 的边框盒承载单行内容，一个盒子搞定原来的
/// "面包屑盒 + 目录信息盒"两个盒子，回收竖向空间。
/// 右侧统计按其实际显示宽度分得独立子区，左侧占剩余区，
/// 二者不重叠——窄终端下左侧面包屑在自身区内截断，右侧统计完整保留。
pub fn render_header(f: &mut Frame, area: Rect, title: &str, left: Vec<Span<'_>>, right: Vec<Span<'_>>) {
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(crate::theme::accent()));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let left_line = Line::from(left);
    let right_line = Line::from(right);

    // 右侧按显示宽度（含 CJK 双宽）分配子区，左侧取剩余，避免叠加覆盖
    let right_w = u16::try_from(right_line.width()).unwrap_or(u16::MAX).min(inner.width);
    let left_w = inner.width - right_w;

    let left_rect = Rect { x: inner.x, width: left_w, ..inner };
    let right_rect = Rect { x: inner.x + left_w, width: right_w, ..inner };

    f.render_widget(Paragraph::new(left_line), left_rect);
    f.render_widget(
        Paragraph::new(right_line).alignment(Alignment::Right),
        right_rect,
    );
}

/// 单行 footer 提示（DarkGray，无边框），复用现有底部 hint 样式。
pub fn render_footer(f: &mut Frame, area: Rect, hint: &str) {
    let para = Paragraph::new(hint).style(Style::default().fg(crate::theme::ink_muted()));
    f.render_widget(para, area);
}

/// 三段布局：[header(Length 3), body(Min), footer(Length 1)]，供各页复用。
#[must_use]
pub fn three_row_layout(area: Rect) -> [Rect; 3] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    [chunks[0], chunks[1], chunks[2]]
}

/// 返回在 `area` 中水平按百分比宽度、垂直按绝对行数居中的矩形。
///
/// 用于帮助/确认等覆盖层：宽度随屏幕缩放，高度按内容行数固定。
#[must_use]
pub fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let height = height.min(area.height);
    // u32 中间运算避免超宽终端(>1365 列)时 u16 乘法溢出
    let width = (u32::from(area.width) * u32::from(percent_x) / 100) as u16;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect { x, y, width, height }
}
/// 在列表区域的右边框列上绘制垂直滚动条。
///
/// 仅当 `total > 可见高度`（内容超出一屏）时才绘制，避免短列表显示满条。
/// `total` 为总行数，`position` 为当前光标行索引。滚动条通过 `Margin{vertical:1}`
/// 内缩，绘制在上下边框之间的右边框列上，不覆盖边框角、不额外占用内容宽度。
pub fn render_scrollbar(f: &mut Frame, area: Rect, total: usize, position: usize) {
    // 可见高度 = 区域高度减去上下边框 2 行
    let visible = (area.height as usize).saturating_sub(2);
    if visible == 0 || total <= visible {
        return; // 内容未超出一屏，不显示滚动条
    }
    let mut sb_state = ScrollbarState::new(total).position(position);
    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None);
    f.render_stateful_widget(
        scrollbar,
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut sb_state,
    );
}
