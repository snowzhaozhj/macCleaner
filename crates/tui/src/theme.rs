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
