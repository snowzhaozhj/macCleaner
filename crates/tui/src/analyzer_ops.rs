//! 分析器（Analyzing / AnalyzingLive）子系统：删除后剪树+导航校正、及两态键盘处理。
//!
//! 剪树/导航校正为纯 `DirNode` 逻辑；键盘处理触碰 `App`/`AppState`。共享符号
//! `resolve_nav_node` / `toggle_marked` / `PAGE_STEP` / `cancel_analyze_to_menu` 保留在 crate 根。

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crossbeam_channel::Receiver;
use crossterm::event::{KeyCode, KeyModifiers};
use humansize::{format_size, DECIMAL};
use mc_core::models::DirNode;
use mc_core::progress::AnalyzeEvent;

use crate::app::{self, App, AppState};
use crate::tree_builder::IncrementalTreeBuilder;
use crate::{cancel_analyze_to_menu, resolve_nav_node, toggle_marked, PAGE_STEP};

/// 递归收集树中被标记路径的 (路径, 大小)；命中标记目录即整体计入、不再深入
pub(crate) fn collect_marked(node: &DirNode, marked: &HashSet<PathBuf>, out: &mut Vec<(PathBuf, u64)>) {
    if marked.contains(&node.path) {
        out.push((node.path.clone(), node.size));
        return;
    }
    for child in &node.children {
        collect_marked(child, marked, out);
    }
}

/// 分析器删除完成：从暂存树剪除已删路径、修正各层大小，校正导航后原地恢复 Analyzing。
pub(crate) fn restore_analyzer_after_delete(
    app: &mut App,
    ret: app::AnalyzerReturn,
    freed: u64,
    count: usize,
    deleted_paths: &[PathBuf],
) {
    let app::AnalyzerReturn {
        tree,
        mut nav_path,
        mut cursor,
        mut cursor_stack,
        deleted,
    } = ret;
    // 仅按**成功**删除的路径剪树：失败项（权限/SIP/占用等）保留在树中，
    // 界面显示的用量与磁盘保持一致，用户仍能看到并重试。
    let failed = deleted.len().saturating_sub(deleted_paths.len());
    let deleted_set: HashSet<PathBuf> = deleted_paths.iter().cloned().collect();
    // 剪枝前记录 nav_path 各层目标的路径快照：剪枝后按**路径**（而非裸索引）恢复导航，
    // 避免删除某祖先层靠前兄弟致索引左移后 nav_path 静默指向另一目录。
    let nav_target_paths = nav_path_target_paths(&tree, &nav_path);
    let mut tree = tree;
    {
        let root = Arc::make_mut(&mut tree);
        prune_paths(root, &deleted_set);
        clamp_nav_after_prune(
            root,
            &nav_target_paths,
            &mut nav_path,
            &mut cursor,
            &mut cursor_stack,
        );
    }
    app.state = AppState::Analyzing {
        tree_root: tree,
        nav_path,
        cursor,
        cursor_stack,
    };
    if failed > 0 {
        app.status_message = Some(format!(
            "已删除 {} 项，释放 {}；{} 项失败，仍保留",
            count,
            format_size(freed, DECIMAL),
            failed
        ));
        return;
    }
    app.status_message = Some(format!(
        "已删除 {} 项，释放 {}",
        count,
        format_size(freed, DECIMAL)
    ));
}

/// 递归剪除 children 中路径命中 `deleted` 的节点，并自底向上按剩余 children 重算目录 size。
pub(crate) fn prune_paths(node: &mut DirNode, deleted: &HashSet<PathBuf>) {
    node.children.retain(|c| !deleted.contains(&c.path));
    for child in &mut node.children {
        if !child.is_file {
            prune_paths(child, deleted);
        }
    }
    if !node.is_file {
        node.size = node.children.iter().map(|c| c.size).sum();
    }
}

/// 剪枝前沿 `nav_path` 逐层解析出每一层目标目录的路径快照，供剪枝后按路径恢复导航。
pub(crate) fn nav_path_target_paths(root: &DirNode, nav_path: &[usize]) -> Vec<PathBuf> {
    let mut node = root;
    let mut paths = Vec::with_capacity(nav_path.len());
    for &idx in nav_path {
        match node.children.get(idx) {
            Some(c) => {
                paths.push(c.path.clone());
                node = c;
            }
            None => break,
        }
    }
    paths
}

/// 剪枝后校正导航：按剪枝前记录的**路径**逐层重新定位索引（而非沿用可能左移的裸索引），
/// 某层目标已删或不再是目录则在该层截断，最后把 cursor 夹回当前层范围。
pub(crate) fn clamp_nav_after_prune(
    root: &DirNode,
    nav_target_paths: &[PathBuf],
    nav_path: &mut Vec<usize>,
    cursor: &mut usize,
    cursor_stack: &mut Vec<usize>,
) {
    let mut node = root;
    let mut new_nav = Vec::with_capacity(nav_target_paths.len());
    for target in nav_target_paths {
        match node
            .children
            .iter()
            .position(|c| !c.is_file && &c.path == target)
        {
            Some(idx) => {
                new_nav.push(idx);
                node = &node.children[idx];
            }
            None => break,
        }
    }
    let valid = new_nav.len();
    *nav_path = new_nav;
    cursor_stack.truncate(valid);
    let current = resolve_nav_node(root, nav_path);
    let len = current.children.len();
    *cursor = if len == 0 { 0 } else { (*cursor).min(len - 1) };
}

/// 磁盘分析器键盘处理（Analyzing 状态，完成后的纯内存导航）
pub(crate) fn handle_analyzer_key(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    if let AppState::Analyzing {
        tree_root,
        nav_path,
        cursor,
        cursor_stack,
        ..
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        let len = current_node.children.len();
        match key {
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
                }
            // 翻页：PageDown / Ctrl+d 下移、PageUp / Ctrl+u 上移
            KeyCode::PageDown => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                }
            }
            KeyCode::PageUp => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                }
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
            }
            // 跳转：Home / g 首行、End / G 末行
            KeyCode::Home | KeyCode::Char('g') => {
                *cursor = 0;
            }
            KeyCode::End | KeyCode::Char('G') => {
                if len > 0 {
                    *cursor = len - 1;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                if let Some(child) = current_node.children.get(*cursor) {
                    if !child.is_file && !child.children.is_empty() {
                        cursor_stack.push(*cursor);
                        nav_path.push(*cursor);
                        *cursor = 0;
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if nav_path.is_empty() {
                    app.back_to_menu();
                } else {
                    nav_path.pop();
                    *cursor = cursor_stack.pop().unwrap_or(0);
                }
            }
            // 标记（统一键位：Space，d 为别名）
            KeyCode::Char(' ' | 'd') => {
                let path = current_node.children.get(*cursor).map(|c| c.path.clone());
                if let Some(p) = path {
                    toggle_marked(&mut app.marked, p);
                }
            }
            KeyCode::Char('x') => {
                let mut list = Vec::new();
                collect_marked(tree_root, &app.marked, &mut list);
                if !list.is_empty() {
                    // 分析器项来自 DirNode，无规则元数据：按路径回查规则证据（evidence_for_path），
                    // 使 Risky 路径（Docker 卷/Xcode Archives 等）经分析器删除时也触发 type-to-confirm；
                    // 未命中任何规则的普通路径默认 Safe、空证据（KTD8）。
                    let items = list
                        .into_iter()
                        .map(|(path, size)| {
                            let (safety, impact, recovery) =
                                mc_core::rules::evidence_for_path(&path).unwrap_or((
                                    mc_core::models::SafetyLevel::Safe,
                                    String::new(),
                                    String::new(),
                                ));
                            crate::app::ConfirmItem {
                                path,
                                size,
                                safety,
                                category: String::new(),
                                impact,
                                recovery,
                            }
                        })
                        .collect();
                    app.confirm_delete = Some(items);
                    app.confirm_scroll = 0;
                }
            }
            _ => {}
        }
    }
}

/// `AnalyzingLive` 状态键盘处理（增量构建中的可导航界面）
pub(crate) fn handle_analyzer_live_key(
    app: &mut App,
    key: KeyCode,
    modifiers: KeyModifiers,
    analyze_rx: &mut Option<Receiver<AnalyzeEvent>>,
    tree_builder: &mut Option<IncrementalTreeBuilder>,
    sort_rx: &mut Option<Receiver<DirNode>>,
) {
    // live 态现在可标记/删除（KTD2）：标记只动 App.marked（路径集合），与正在生长的树
    // 完全独立；按路径经 size_desc_order 置换翻译 + .get() 兜底，把实时重排的 TOCTOU
    // 降为安全 no-op（曾因"按位置标记误标最大项"而被 75eaa4f 一刀切封锁，此处按路径根治）。
    // 删除（x）见 confirm_accept：提交时先 finalize 部分树再删（KTD1）。
    // 先提取需要的字段进行操作
    if let AppState::AnalyzingLive {
        tree_root,
        nav_path,
        cursor,
        cursor_stack,
        user_navigated,
        ..
    } = &mut app.state
    {
        let current_node = resolve_nav_node(tree_root, nav_path);
        let len = current_node.children.len();
        match key {
            KeyCode::Up | KeyCode::Char('k')
                if *cursor > 0 => {
                    *cursor -= 1;
                    *user_navigated = true;
                }
            KeyCode::Down | KeyCode::Char('j')
                if !current_node.children.is_empty() && *cursor < current_node.children.len() - 1 => {
                    *cursor += 1;
                    *user_navigated = true;
                }
            // 翻页：PageDown / Ctrl+d 下移、PageUp / Ctrl+u 上移（均置 user_navigated）
            KeyCode::PageDown => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                    *user_navigated = true;
                }
            }
            KeyCode::PageUp => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
                *user_navigated = true;
            }
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                if len > 0 {
                    *cursor = (*cursor + PAGE_STEP).min(len - 1);
                    *user_navigated = true;
                }
            }
            KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                *cursor = cursor.saturating_sub(PAGE_STEP);
                *user_navigated = true;
            }
            // 跳转：Home / g 首行、End / G 末行
            KeyCode::Home | KeyCode::Char('g') => {
                *cursor = 0;
                *user_navigated = true;
            }
            KeyCode::End | KeyCode::Char('G') => {
                if len > 0 {
                    *cursor = len - 1;
                    *user_navigated = true;
                }
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                // cursor 是显示序位置，经 order 映回底层存储索引再入 nav_path
                let order = crate::ui::analyzer::size_desc_order(&current_node.children);
                if let Some(&stored_idx) = order.get(*cursor) {
                    if current_node.children.get(stored_idx).is_some_and(|c| !c.is_file) {
                        *user_navigated = true;
                        cursor_stack.push(*cursor);
                        nav_path.push(stored_idx);
                        *cursor = 0;
                    }
                }
            }
            KeyCode::Backspace | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                if nav_path.is_empty() {
                    cancel_analyze_to_menu(app, analyze_rx, tree_builder, sort_rx);
                } else {
                    *user_navigated = true;
                    nav_path.pop();
                    *cursor = cursor_stack.pop().unwrap_or(0);
                }
            }
            // 标记（Space/d）：cursor 是显示序，经 path_at_display_index 置换映回存储索引取 path
            // （一律 .get()，把流式重排的 TOCTOU 降为 no-op，KTD2）。不移光标、不改 user_navigated。
            KeyCode::Char(' ' | 'd') => {
                let path = crate::ui::analyzer::path_at_display_index(&current_node.children, *cursor);
                if let Some(p) = path {
                    toggle_marked(&mut app.marked, p);
                }
            }
            // 删除（x）：与 Analyzing 同款按路径回查规则证据（Risky 触发 type-to-confirm）；
            // 提交在 confirm_accept 走 finalize→delete（KTD1）。
            KeyCode::Char('x') => {
                let mut list = Vec::new();
                collect_marked(tree_root, &app.marked, &mut list);
                if !list.is_empty() {
                    let items = list
                        .into_iter()
                        .map(|(path, size)| {
                            let (safety, impact, recovery) =
                                mc_core::rules::evidence_for_path(&path).unwrap_or((
                                    mc_core::models::SafetyLevel::Safe,
                                    String::new(),
                                    String::new(),
                                ));
                            crate::app::ConfirmItem {
                                path,
                                size,
                                safety,
                                category: String::new(),
                                impact,
                                recovery,
                            }
                        })
                        .collect();
                    app.confirm_delete = Some(items);
                    app.confirm_scroll = 0;
                }
            }
            _ => {}
        }
    }
}
