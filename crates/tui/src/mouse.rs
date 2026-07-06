//! 鼠标/触控板事件处理：滚轮步进 + 左键点击定位/标记。
//!
//! 与渲染同源做命中测试（重跑纯布局函数），故重度依赖 `crate::ui::chrome` 的布局辅助。
//! 共享符号 `toggle_marked` / `resolve_nav_node` / `CONFIRM_ROWS_PER_ITEM` 保留在 `crate` 根。

use std::path::PathBuf;

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::{App, AppState, FlatRow};
use crate::{resolve_nav_node, toggle_marked, CONFIRM_ROWS_PER_ITEM};

/// 滚轮/触控板单次滚动移动的行数（固定步进，跨终端一致，取代终端惯性放大导致的"飞行"）。
const MOUSE_SCROLL_STEP: usize = 3;

/// 处理鼠标事件：滚轮步进 + 左键点击（定位光标 + 切换标记）。
///
/// `term_area` 为事件时的终端全区，用于重算与渲染同源的 list 布局做命中测试
/// （渲染函数签名为 `&App` 不可变，改成 `&mut` 过度侵入，故按当前尺寸重跑纯布局函数）。
pub(crate) fn handle_mouse(app: &mut App, mouse: MouseEvent, term_area: Rect) {
    // Cleaning 态吞掉所有鼠标（对齐键盘"清理中不响应"守卫，避免中断删除）。
    if matches!(app.state, AppState::Cleaning { .. }) {
        return;
    }
    // 删除确认覆盖层：仅滚轮调 confirm_scroll，点击忽略（type-to-confirm 保持键盘）。
    if app.confirm_delete.is_some() {
        let cap = app
            .confirm_delete
            .as_ref()
            .map_or(0, Vec::len)
            .saturating_mul(CONFIRM_ROWS_PER_ITEM);
        match mouse.kind {
            MouseEventKind::ScrollDown => {
                app.confirm_scroll = (app.confirm_scroll + MOUSE_SCROLL_STEP).min(cap);
            }
            MouseEventKind::ScrollUp => {
                app.confirm_scroll = app.confirm_scroll.saturating_sub(MOUSE_SCROLL_STEP);
            }
            _ => {}
        }
        return;
    }
    // 帮助/过滤覆盖层：忽略鼠标，保持键盘语义。
    if app.show_help || app.filter_active {
        return;
    }

    match mouse.kind {
        MouseEventKind::ScrollDown => mouse_scroll(app, true),
        MouseEventKind::ScrollUp => mouse_scroll(app, false),
        MouseEventKind::Down(MouseButton::Left) => {
            mouse_click(app, term_area, mouse.column, mouse.row);
        }
        _ => {}
    }
}

/// 对显示序/存储序光标做步进 clamp；返回是否有变化（供 Live 置 `user_navigated`）。
pub(crate) fn scroll_cursor(cursor: &mut usize, len: usize, down: bool) -> bool {
    if len == 0 {
        return false;
    }
    let new = if down {
        (*cursor + MOUSE_SCROLL_STEP).min(len - 1)
    } else {
        cursor.saturating_sub(MOUSE_SCROLL_STEP)
    };
    let changed = new != *cursor;
    *cursor = new;
    changed
}

/// 滚轮移动光标（按当前状态）。语义与键盘方向键一致（含 clamp / 分隔行跳过）。
fn mouse_scroll(app: &mut App, down: bool) {
    // Results/Scanning 复用既有翻页方法（含分隔行跳过 + clamp，且每次只 build 一次 flat_rows，
    // 避免逐行 move ×N 重复重建）——需整体 `&mut app`，故先用 matches! 判定（不持借用）再处理，
    // 避免与下方 `&mut app.state` 借用冲突。
    if matches!(app.state, AppState::Results | AppState::Scanning { .. }) {
        if down {
            app.move_cursor_page_down(MOUSE_SCROLL_STEP);
        } else {
            app.move_cursor_page_up(MOUSE_SCROLL_STEP);
        }
        return;
    }
    match &mut app.state {
        AppState::Analyzing {
            tree_root,
            nav_path,
            cursor,
            ..
        } => {
            let len = resolve_nav_node(tree_root, nav_path).children.len();
            scroll_cursor(cursor, len, down);
        }
        AppState::AnalyzingLive {
            tree_root,
            nav_path,
            cursor,
            user_navigated,
            ..
        } => {
            let len = resolve_nav_node(tree_root, nav_path).children.len();
            if scroll_cursor(cursor, len, down) {
                *user_navigated = true;
            }
        }
        _ => {}
    }
}

/// 命中测试：全局 (col,row) → 列表数据的显示序索引。边框/区域外/超出数据 → None。
/// `cursor`/`total` 用于复算与渲染**同源**的 `window_start`（`chrome::window_start`），
/// 保证"点击落在第几行"与"实际画在第几行"一致。
pub(crate) fn hit_row(list_area: Rect, col: u16, row: u16, cursor: usize, total: usize) -> Option<usize> {
    if total == 0 {
        return None;
    }
    // 水平：落在列表区域外忽略（不细分列，整行选中）。
    if col < list_area.x || col >= list_area.x.saturating_add(list_area.width) {
        return None;
    }
    // 垂直：排除上下边框（各 1 行）。
    let top = list_area.y;
    let bottom = list_area.y.saturating_add(list_area.height.saturating_sub(1));
    if row <= top || row >= bottom {
        return None;
    }
    let visible_height = crate::ui::chrome::list_visible_height(list_area);
    if visible_height == 0 {
        return None;
    }
    let cursor = cursor.min(total - 1);
    let window_start = crate::ui::chrome::window_start(cursor, visible_height);
    let visible_row = (row - top - 1) as usize;
    let idx = window_start + visible_row;
    // 点击落在最后一项之后的空白区（idx>=total）视为 no-op。
    if idx < total {
        Some(idx)
    } else {
        None
    }
}

/// 左键点击：命中测试定位到项 → 移动光标 + 切换标记（分隔行/区域外 no-op）。
fn mouse_click(app: &mut App, term_area: Rect, col: u16, row: u16) {
    let [_, body, _] = crate::ui::chrome::three_row_layout(term_area);

    // Results/Scanning：走 flat_rows + toggle_selection（需整体 &mut app，用 matches! 判分支）。
    if matches!(app.state, AppState::Results | AppState::Scanning { .. }) {
        // Results 的 body 再分列表+详情；Scanning 用整块 body 作列表（无详情面板）。
        let list_area = if matches!(app.state, AppState::Results) {
            crate::ui::results::split_body(body).0
        } else {
            body
        };
        let flat_rows = app.build_flat_rows();
        if let Some(idx) = hit_row(list_area, col, row, app.result_cursor, flat_rows.len()) {
            if let Some(fr) = flat_rows.get(idx) {
                if matches!(fr, FlatRow::Separator { .. }) {
                    return; // 分隔行不可选
                }
                let fr = fr.clone();
                app.result_cursor = idx;
                app.toggle_selection(&fr);
            }
        }
        return;
    }

    // Analyzer/Live：命中后先 clone path（结束对 tree_root 的借用）再改 marked。
    let clicked_path: Option<PathBuf> = match &mut app.state {
        AppState::Analyzing {
            tree_root,
            nav_path,
            cursor,
            ..
        } => {
            let node = resolve_nav_node(tree_root, nav_path);
            let total = node.children.len();
            if let Some(idx) = hit_row(body, col, row, *cursor, total) {
                node.children.get(idx).map(|c| {
                    let p = c.path.clone();
                    *cursor = idx;
                    p
                })
            } else {
                None
            }
        }
        AppState::AnalyzingLive {
            tree_root,
            nav_path,
            cursor,
            user_navigated,
            ..
        } => {
            let node = resolve_nav_node(tree_root, nav_path);
            let total = node.children.len();
            if let Some(idx) = hit_row(body, col, row, *cursor, total) {
                // idx 是显示序，经 size_desc_order 映回存储索引再取 path（与键盘标记同源）。
                let path = crate::ui::analyzer::path_at_display_index(&node.children, idx);
                if path.is_some() {
                    *cursor = idx;
                    *user_navigated = true;
                }
                path
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(p) = clicked_path {
        toggle_marked(&mut app.marked, p);
    }
}
