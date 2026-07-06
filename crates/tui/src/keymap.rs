//! 键位注册表：`?` 帮助覆盖层与底部 hint 的**单一事实来源**。
//!
//! 每个界面的可用键位只在此声明一次，help 覆盖层与 footer 都从这里生成，
//! 二者永不漂移。新增/调整键位只需改这一处。

use crate::app::AppState;

/// 单条键位提示：`keys` 为按键、`desc` 为说明、`priority` 为 footer 拥挤时的保留优先级。
///
/// `priority`：0=最高（永不剔除），数值越大越先被 footer 降级剔除。分配约定：
/// 0=删除 `x`/帮助 `?`/退出返回；1=标记/确认/过滤/全选；2=进入展开；3=方向/翻页/跳转。
#[derive(Clone, Copy)]
pub struct KeyHint {
    pub keys: &'static str,
    pub desc: &'static str,
    pub priority: u8,
}

/// 全局键位（所有界面通用），拼接在各界面专属键位之后。
/// 注：退出/返回语义按界面分层（见各 arm 的 q），故不放全局，只保留帮助键。
const GLOBAL: &[KeyHint] = &[
    KeyHint { keys: "?", desc: "帮助", priority: 0 },
];

/// 返回某界面的**全部**可用键位（专属 + 全局），供 help 覆盖层展示。
///
/// 统一约定（跨 Results 与 Analyzer 一致）：`Space`=标记(不移光标)、`x`=删除已标记、
/// `Enter`=进入/展开(**永不删除**)；`q` 在菜单=退出程序，在子界面=返回菜单。
pub fn hints_for(state: &AppState) -> Vec<KeyHint> {
    let specific: &[KeyHint] = match state {
        AppState::Menu => &[
            KeyHint { keys: "↑↓ / jk", desc: "选择", priority: 3 },
            KeyHint { keys: "Enter", desc: "执行", priority: 1 },
            KeyHint { keys: "q / Esc", desc: "退出程序", priority: 0 },
        ],
        AppState::Scanning { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动", priority: 3 },
            KeyHint { keys: "h / l", desc: "折叠 / 展开", priority: 3 },
            KeyHint { keys: "Space / d", desc: "标记", priority: 1 },
            KeyHint { keys: "a", desc: "全选安全项", priority: 1 },
            KeyHint { keys: "Tab", desc: "展开/折叠", priority: 2 },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)", priority: 0 },
            // 与 Results 一致：x 删除与 ? 帮助是 footer 两条硬保留(priority 0)，取消返回让位为 1。
            KeyHint { keys: "Esc / Backspace / q", desc: "取消扫描并返回", priority: 1 },
        ],
        AppState::Results => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动", priority: 3 },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页", priority: 3 },
            KeyHint { keys: "g / G", desc: "首行 / 末行", priority: 3 },
            KeyHint { keys: "/", desc: "过滤", priority: 1 },
            KeyHint { keys: "Space / d", desc: "标记", priority: 1 },
            KeyHint { keys: "Tab / Enter", desc: "展开/折叠", priority: 2 },
            KeyHint { keys: "h / l", desc: "折叠 / 展开", priority: 3 },
            KeyHint { keys: "a", desc: "全选安全项", priority: 1 },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)", priority: 0 },
            // 返回/清过滤降为 1：x 删除与 ? 帮助是 footer 的两条硬保留（priority 0），
            // 三条 priority-0 在 80 列放不下时，返回类先让位（KTD4 的 x/? 可见性契约）。
            KeyHint { keys: "Esc / Backspace", desc: "清过滤 / 返回菜单", priority: 1 },
            KeyHint { keys: "q", desc: "返回菜单", priority: 1 },
        ],
        AppState::Cleaning { .. } => &[KeyHint { keys: "", desc: "清理中，请稍候...", priority: 0 }],
        AppState::Done { .. } => &[
            KeyHint { keys: "Enter / q", desc: "返回菜单", priority: 0 },
        ],
        AppState::Analyzing { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动", priority: 3 },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页", priority: 3 },
            KeyHint { keys: "g / G", desc: "首行 / 末行", priority: 3 },
            KeyHint { keys: "Enter / l", desc: "进入目录", priority: 2 },
            KeyHint { keys: "Backspace / h", desc: "返回上级", priority: 2 },
            KeyHint { keys: "Space / d", desc: "标记", priority: 1 },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)", priority: 0 },
            KeyHint { keys: "q", desc: "返回菜单", priority: 1 },
        ],
        AppState::AnalyzingLive { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动", priority: 3 },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页", priority: 3 },
            KeyHint { keys: "g / G", desc: "首行 / 末行", priority: 3 },
            KeyHint { keys: "Enter / l", desc: "进入目录", priority: 2 },
            KeyHint { keys: "Backspace / h", desc: "返回上级", priority: 2 },
            KeyHint { keys: "Space / d", desc: "标记", priority: 1 },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)", priority: 0 },
            KeyHint { keys: "q", desc: "返回菜单", priority: 1 },
        ],
        AppState::Sorting => &[KeyHint { keys: "Esc / Backspace", desc: "取消并返回", priority: 0 }],
    };

    specific.iter().chain(GLOBAL).copied().collect()
}

/// 一条 hint 的展示文本（空 keys 只显示 desc）。
fn hint_text(h: &KeyHint) -> String {
    if h.keys.is_empty() {
        h.desc.to_string()
    } else {
        format!("{} {}", h.keys, h.desc)
    }
}

/// 从键位表生成适配 `max_width`（显示宽度）的单行 footer。
///
/// 装配全量后若溢出，按 `priority` **从低到高（数值大者先）整条剔除**——绝不留半截文字，
/// 保证 `x 删除`/`? 帮助`（priority 0）在任何宽度下都可见（KTD4）。help 覆盖层不受影响。
pub fn footer_line(state: &AppState, max_width: usize) -> String {
    let hints = hints_for(state);
    let mut keep = vec![true; hints.len()];

    loop {
        let line = assemble(&hints, &keep);
        if crate::ui::text::display_width(&line) <= max_width {
            return line;
        }
        // 在当前保留项中挑 priority 数值最大（最低优先级）、最靠右的一条剔除。
        let victim = hints
            .iter()
            .enumerate()
            .filter(|(i, _)| keep[*i])
            .max_by_key(|(i, h)| (h.priority, *i))
            .map(|(i, _)| i);
        match victim {
            Some(i) => keep[i] = false,
            None => return line, // 已无可剔除，交由渲染层截断
        }
        if keep.iter().all(|k| !k) {
            return assemble(&hints, &keep);
        }
    }
}

/// 按保留标记装配 footer 文本（前导空格 + ` | ` 分隔）。
fn assemble(hints: &[KeyHint], keep: &[bool]) -> String {
    let parts: Vec<String> = hints
        .iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(h, _)| hint_text(h))
        .collect();
    format!(" {}", parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::text::display_width;

    #[test]
    fn footer_keeps_priority_zero_when_narrow() {
        // 80 列（实测最窄终端）下方向/翻页等低优先项被剔除，但 x 删除 与 ? 帮助（priority 0）仍可见。
        let line = footer_line(&AppState::Results, 80);
        assert!(display_width(&line) <= 80, "应适配 80 列：{}", display_width(&line));
        assert!(line.contains('x'), "x 删除 必须保留：{line}");
        assert!(line.contains('?'), "? 帮助 必须保留：{line}");
    }

    #[test]
    fn footer_full_when_wide() {
        // 足够宽时全量展示，方向键提示也在。
        let line = footer_line(&AppState::Results, 200);
        assert!(line.contains("移动"));
        assert!(line.contains("翻页"));
        assert!(line.contains("删除"));
    }

    #[test]
    fn footer_drops_low_priority_first() {
        // 中等宽度：方向/翻页(3) 先于 标记(1)、删除(0) 被剔除。
        let line = footer_line(&AppState::Results, 60);
        assert!(display_width(&line) <= 60, "应适配宽度：{}", display_width(&line));
        assert!(line.contains('x'), "删除应保留");
        assert!(!line.contains("翻页") || !line.contains("首行"), "低优先项应先被剔除");
    }

    fn scanning_state() -> AppState {
        AppState::Scanning {
            progress_text: String::new(),
            rule_current: 0,
            rule_total: 0,
            rule_name: String::new(),
        }
    }

    fn analyzing_live_state() -> AppState {
        use mc_core::models::DirNode;
        use std::path::PathBuf;
        AppState::AnalyzingLive {
            tree_root: DirNode::new_dir(PathBuf::from("/"), "/".into()),
            nav_path: Vec::new(),
            cursor: 0,
            cursor_stack: Vec::new(),
            file_count: 0,
            total_size: 0,
            user_navigated: false,
        }
    }

    #[test]
    fn scanning_footer_exposes_mark_and_delete() {
        // 扫描态现在可标记/删除：宽屏应展示，且 x/? 是窄屏硬保留。
        let wide = footer_line(&scanning_state(), 200);
        assert!(wide.contains("标记"), "扫描态应展示标记：{wide}");
        assert!(wide.contains("删除"), "扫描态应展示删除：{wide}");
        let narrow = footer_line(&scanning_state(), 80);
        assert!(display_width(&narrow) <= 80);
        assert!(narrow.contains('x'), "x 删除窄屏硬保留：{narrow}");
        assert!(narrow.contains('?'), "? 帮助窄屏硬保留：{narrow}");
    }

    #[test]
    fn analyzing_live_footer_exposes_mark_and_delete_not_locked_hint() {
        // live 态恢复标记/删除，不再展示"扫描完成后可标记与删除"的封锁提示。
        let line = footer_line(&analyzing_live_state(), 200);
        assert!(line.contains("标记"), "live 应展示标记：{line}");
        assert!(line.contains("删除"), "live 应展示删除：{line}");
        assert!(!line.contains("扫描完成后"), "不应再有封锁提示：{line}");
    }

    #[test]
    fn results_hints_include_hl_folding() {
        // h/l 是 priority-3（窄屏会被裁），故直接查键位表而非 footer_line。
        let hints = hints_for(&AppState::Results);
        assert!(
            hints.iter().any(|h| h.keys.contains("h / l")),
            "Results 键位表应含 h/l 折叠展开"
        );
    }
}
