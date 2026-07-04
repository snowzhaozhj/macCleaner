//! 键位注册表：`?` 帮助覆盖层与底部 hint 的**单一事实来源**。
//!
//! 每个界面的可用键位只在此声明一次，help 覆盖层与 footer 都从这里生成，
//! 二者永不漂移。新增/调整键位只需改这一处。

use crate::app::AppState;

/// 单条键位提示：`keys` 为按键、`desc` 为说明。
#[derive(Clone, Copy)]
pub struct KeyHint {
    pub keys: &'static str,
    pub desc: &'static str,
}

/// 全局键位（所有界面通用），拼接在各界面专属键位之后。
/// 注：退出/返回语义按界面分层（见各 arm 的 q），故不放全局，只保留帮助键。
const GLOBAL: &[KeyHint] = &[
    KeyHint { keys: "?", desc: "帮助" },
];

/// 返回某界面的**全部**可用键位（专属 + 全局），供 help 覆盖层展示。
///
/// 统一约定（跨 Results 与 Analyzer 一致）：`Space`=标记(不移光标)、`x`=删除已标记、
/// `Enter`=进入/展开(**永不删除**)；`q` 在菜单=退出程序，在子界面=返回菜单。
pub fn hints_for(state: &AppState) -> Vec<KeyHint> {
    let specific: &[KeyHint] = match state {
        AppState::Menu => &[
            KeyHint { keys: "↑↓ / jk", desc: "选择" },
            KeyHint { keys: "Enter", desc: "执行" },
            KeyHint { keys: "q / Esc", desc: "退出程序" },
        ],
        AppState::Scanning { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "Tab", desc: "展开/折叠" },
            KeyHint { keys: "Esc / Backspace / q", desc: "取消扫描并返回" },
        ],
        AppState::Results => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页" },
            KeyHint { keys: "g / G", desc: "首行 / 末行" },
            KeyHint { keys: "/", desc: "过滤" },
            KeyHint { keys: "Space / d", desc: "标记" },
            KeyHint { keys: "Tab / Enter", desc: "展开/折叠" },
            KeyHint { keys: "a", desc: "全选安全项" },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)" },
            KeyHint { keys: "Esc / Backspace", desc: "清过滤 / 返回菜单" },
            KeyHint { keys: "q", desc: "返回菜单" },
        ],
        AppState::Cleaning { .. } => &[KeyHint { keys: "", desc: "清理中，请稍候..." }],
        AppState::Done { .. } => &[
            KeyHint { keys: "Enter", desc: "返回菜单" },
            KeyHint { keys: "q", desc: "返回菜单" },
        ],
        AppState::Analyzing { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页" },
            KeyHint { keys: "g / G", desc: "首行 / 末行" },
            KeyHint { keys: "Enter / l", desc: "进入目录" },
            KeyHint { keys: "Backspace / h", desc: "返回上级" },
            KeyHint { keys: "Space / d", desc: "标记" },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)" },
            KeyHint { keys: "q", desc: "返回菜单" },
        ],
        AppState::AnalyzingLive { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页" },
            KeyHint { keys: "g / G", desc: "首行 / 末行" },
            KeyHint { keys: "Enter / l", desc: "进入目录" },
            KeyHint { keys: "Backspace / h", desc: "返回上级" },
            KeyHint { keys: "", desc: "扫描完成后可标记与删除" },
            KeyHint { keys: "q", desc: "返回菜单" },
        ],
        AppState::Sorting => &[KeyHint { keys: "Esc / Backspace", desc: "取消并返回" }],
    };

    specific.iter().chain(GLOBAL).copied().collect()
}

/// 从键位表生成紧凑的单行 footer 提示，与 help 覆盖层天然同步。
pub fn footer_line(state: &AppState) -> String {
    let parts: Vec<String> = hints_for(state)
        .iter()
        .map(|h| {
            if h.keys.is_empty() {
                h.desc.to_string()
            } else {
                format!("{} {}", h.keys, h.desc)
            }
        })
        .collect();
    format!(" {}", parts.join(" | "))
}
