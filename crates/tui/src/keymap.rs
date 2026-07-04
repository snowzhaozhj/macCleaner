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
const GLOBAL: &[KeyHint] = &[
    KeyHint { keys: "?", desc: "帮助" },
    KeyHint { keys: "q", desc: "退出程序" },
];

/// 返回某界面的**全部**可用键位（专属 + 全局），供 help 覆盖层展示。
pub fn hints_for(state: &AppState) -> Vec<KeyHint> {
    let specific: &[KeyHint] = match state {
        AppState::Menu => &[
            KeyHint { keys: "↑↓ / jk", desc: "选择" },
            KeyHint { keys: "Enter", desc: "执行" },
        ],
        AppState::Scanning { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "Tab", desc: "展开/折叠" },
            KeyHint { keys: "Esc / Backspace", desc: "取消扫描并返回" },
        ],
        AppState::Results => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页" },
            KeyHint { keys: "g / G", desc: "首行 / 末行" },
            KeyHint { keys: "/", desc: "过滤" },
            KeyHint { keys: "Space", desc: "选择" },
            KeyHint { keys: "Tab", desc: "展开/折叠" },
            KeyHint { keys: "a", desc: "全选安全项" },
            KeyHint { keys: "Enter", desc: "确认清理" },
            KeyHint { keys: "Esc / Backspace", desc: "清过滤 / 返回菜单" },
        ],
        AppState::Cleaning { .. } => &[KeyHint { keys: "", desc: "清理中，请稍候..." }],
        AppState::Done { .. } => &[KeyHint { keys: "Enter", desc: "返回菜单" }],
        AppState::Analyzing { .. } | AppState::AnalyzingLive { .. } => &[
            KeyHint { keys: "↑↓ / jk", desc: "移动" },
            KeyHint { keys: "PgUp/PgDn / ^u^d", desc: "翻页" },
            KeyHint { keys: "g / G", desc: "首行 / 末行" },
            KeyHint { keys: "Enter / l", desc: "进入目录" },
            KeyHint { keys: "Backspace / h", desc: "返回上级" },
            KeyHint { keys: "d", desc: "标记删除" },
            KeyHint { keys: "x", desc: "删除已标记(移废纸篓)" },
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
