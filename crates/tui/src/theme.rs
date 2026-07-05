//! 主题与可访问性：颜色门控（`NO_COLOR`）+ 安全等级的形状编码。
//!
//! - **`NO_COLOR`**：遵循 <https://no-color.org/> 约定——只要环境变量 `NO_COLOR`
//!   存在且非空，即禁用一切前景/边框色（返回 `Color::Reset` 走终端默认前景），
//!   光标高亮改用反显（`REVERSED`）而非背景色，保证无色环境下仍可用。
//! - **形状编码**：安全等级除颜色外再叠加 `●/▲/✕` 字形，色盲用户与无色终端
//!   均可分辨（不把颜色作为唯一信息通道）。

use mc_core::models::SafetyLevel;
use ratatui::style::{Color, Modifier, Style};
use std::sync::OnceLock;

static NO_COLOR: OnceLock<bool> = OnceLock::new();

/// 启动时读取一次 `NO_COLOR` 环境变量。幂等：重复调用不改变首次结果。
pub fn init() {
    let disabled = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let _ = NO_COLOR.set(disabled);
}

/// 当前是否禁用颜色。未 `init` 时按启用颜色处理（返回 false）。
pub fn no_color() -> bool {
    *NO_COLOR.get().unwrap_or(&false)
}

/// 颜色门控：`NO_COLOR` 时一律回退到终端默认前景色。
#[inline]
pub fn c(color: Color) -> Color {
    if no_color() {
        Color::Reset
    } else {
        color
    }
}

/// 安全等级 → 颜色（已过门控）。
pub fn safety_color(safety: SafetyLevel) -> Color {
    c(match safety {
        SafetyLevel::Safe => Color::Green,
        SafetyLevel::Moderate => Color::Yellow,
        SafetyLevel::Risky => Color::Red,
    })
}

/// 安全等级 → 形状字形（不依赖颜色的第二信息通道）。
pub fn safety_symbol(safety: SafetyLevel) -> &'static str {
    match safety {
        SafetyLevel::Safe => "●",
        SafetyLevel::Moderate => "▲",
        SafetyLevel::Risky => "✕",
    }
}

/// 安全等级 → 中文标签（文字信息通道，`NO_COLOR`/色盲下与符号并存）。
pub fn safety_label(safety: SafetyLevel) -> &'static str {
    match safety {
        SafetyLevel::Safe => "安全",
        SafetyLevel::Moderate => "中等",
        SafetyLevel::Risky => "危险",
    }
}

/// 光标高亮样式：有色时用深灰背景，`NO_COLOR` 时用反显，二者都保持可见。
pub fn cursor_highlight(base: Style) -> Style {
    if no_color() {
        base.add_modifier(Modifier::REVERSED)
    } else {
        base.bg(Color::DarkGray)
    }
}

// ─── 语义 Token 层 ───────────────────────────────────────────────────────────
//
// 颜色以**语义角色**命名，与渲染后端解耦（见根目录 `DESIGN.md` §1）。当前 TUI 把
// 每个角色实现为 16 色 ANSI；未来桌面端把同一角色实现为 OKLCH。角色不变，值随后端变。
// 各 UI 文件只应引用这些语义函数，不再散落硬编码 `Color::`。所有返回值已过 `c()` 门控。
//
// 注意几处**语义重叠**：TUI 中 `success`/`safety_color(Safe)` 同为绿、
// `activity`/`warning`/`safety_color(Moderate)` 同为黄、`danger`/`safety_color(Risky)`
// 同为红——因 ANSI 只有 8 个基础色被迫共用。它们仍是**独立语义函数**，桌面端应在同
// 色相家族内用明度/彩度把它们区分开（详见 DESIGN.md §1.2）。

// ── 结构 / 中性角色 ──
/// 主前景：默认文字、激活项、文件名。
pub fn ink() -> Color {
    c(Color::White)
}
/// 次要信息：标签、大小/百分比列、描述、页脚、分隔符。
pub fn ink_muted() -> Color {
    c(Color::DarkGray)
}
/// 主交互色：选中 / 焦点 / 默认面板边框 / 信息计数（= `state.info`）。
pub fn accent() -> Color {
    c(Color::Cyan)
}
/// 分析器（Analyze 模式）专属次强调：列表边框 + 体积条。
pub fn accent_explore() -> Color {
    c(Color::Blue)
}
/// 次要边框：详情面板等非强调容器。
pub fn border_subtle() -> Color {
    c(Color::DarkGray)
}

// ── 状态角色 ──
/// 正向：已释放 / 总大小 / 完成 / 正向数量强调。
pub fn success() -> Color {
    c(Color::Green)
}
/// 进行中：扫描 / 清理 / 排序 / spinner。
pub fn activity() -> Color {
    c(Color::Yellow)
}
/// 警示（非进行中）：确认标题、隐藏项提示、大文件高亮、type-to-confirm 输入。
pub fn warning() -> Color {
    c(Color::Yellow)
}
/// 破坏性：删除动作、待删标记、取消。
pub fn danger() -> Color {
    c(Color::Red)
}

/// 状态提示条（toast）样式：反色高亮 + 加粗，`NO_COLOR` 下回退终端默认色但保留加粗。
pub fn toast_style() -> Style {
    Style::default()
        .fg(c(Color::Black))
        .bg(c(Color::Yellow))
        .add_modifier(Modifier::BOLD)
}
