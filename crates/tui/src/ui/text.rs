//! 显示宽度感知的文本裁剪工具（KTD3/KTD5/KTD9 共享）。
//!
//! 终端渲染按**显示宽度**（CJK 占 2 列）而非 char 数计算，按 char 数截断会在窄
//! 终端下顶破边框或切半宽字。本模块统一提供按 `unicode-width` 显示宽度的：
//! - `ellipsize_middle`：中段省略（保头保尾），路径/面包屑用；
//! - `truncate_end`：尾部截断补 `…`，header/help 单行用；
//! - `abbreviate_home` / `ellipsize_path`：home 前缀缩写为 `~` 后再中段省略；
//! - `wrap_by_width`：按显示宽度贪心换行（Risky 后果句在确认框内 wrap 用）。

use ratatui::style::Style;
use ratatui::text::Span;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// 字符串的终端显示宽度（CJK 双宽）。
#[must_use]
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 取 `s` 的前缀，累计显示宽度不超过 `budget`。
fn take_prefix(s: &str, budget: usize) -> String {
    let mut w = 0;
    let mut out = String::new();
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > budget {
            break;
        }
        w += cw;
        out.push(ch);
    }
    out
}

/// 取 `s` 的后缀，累计显示宽度不超过 `budget`。
fn take_suffix(s: &str, budget: usize) -> String {
    let mut w = 0;
    let mut rev: Vec<char> = Vec::new();
    for ch in s.chars().rev() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > budget {
            break;
        }
        w += cw;
        rev.push(ch);
    }
    rev.iter().rev().collect()
}

/// 中段省略：显示宽度超过 `max_width` 时保头保尾、中间折叠为 `…`。
/// 头尾预算均分（奇数余量给尾部——路径尾部信息量更大）。
#[must_use]
pub fn ellipsize_middle(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let budget = max_width - 1; // 省略号占 1 列
    let head_budget = budget / 2;
    let tail_budget = budget - head_budget;
    let head = take_prefix(s, head_budget);
    let tail = take_suffix(s, tail_budget);
    format!("{head}…{tail}")
}

/// 尾部截断：显示宽度超过 `max_width` 时截头补 `…`。
#[must_use]
pub fn truncate_end(s: &str, max_width: usize) -> String {
    if display_width(s) <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let head = take_prefix(s, max_width - 1);
    format!("{head}…")
}

/// 把 `s` 裁剪/右填充到恰好 `width` 显示宽度：超宽尾部截断补 `…`，不足右补空格。
/// 用于表格列对齐（`{:<N}` 按 char 数填充会因 CJK 双宽错位，KTD9）。
#[must_use]
pub fn fit_width(s: &str, width: usize) -> String {
    let t = truncate_end(s, width);
    let pad = width.saturating_sub(display_width(&t));
    format!("{t}{}", " ".repeat(pad))
}

/// 把 home 前缀缩写为 `~`（不在 home 下的绝对路径原样返回）。
#[must_use]
pub fn abbreviate_home(path: &Path) -> String {
    let home = mc_core::platform::get_home_dir();
    match path.strip_prefix(&home) {
        Ok(rel) if rel.as_os_str().is_empty() => "~".to_string(),
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

/// 路径专用：先缩写 home，再按显示宽度中段省略。
#[must_use]
pub fn ellipsize_path(path: &Path, max_width: usize) -> String {
    ellipsize_middle(&abbreviate_home(path), max_width)
}

/// 对一串 `Span`（如面包屑：根 / … / 当前层）按显示宽度中段省略，保头保尾、
/// 保留各 span 的样式，中间插入 `…`。用于 header 左区：既加省略号又保证最深层段可见
/// （尾部预算优先保留末段），对齐 DESIGN.md §5.2（KTD5 header + 面包屑）。
#[must_use]
pub fn ellipsize_spans_middle(spans: &[Span<'_>], max_width: usize, ellipsis_style: Style) -> Vec<Span<'static>> {
    let total: usize = spans.iter().map(|s| display_width(s.content.as_ref())).sum();
    let owned = |s: &Span<'_>| Span::styled(s.content.as_ref().to_string(), s.style);
    if total <= max_width {
        return spans.iter().map(owned).collect();
    }
    if max_width <= 1 {
        return vec![Span::styled("…".to_string(), ellipsis_style)];
    }
    let budget = max_width - 1;
    let head_budget = budget / 2;
    let tail_budget = budget - head_budget;

    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for s in spans {
        if used >= head_budget {
            break;
        }
        let w = display_width(s.content.as_ref());
        if used + w <= head_budget {
            out.push(owned(s));
            used += w;
        } else {
            let part = take_prefix(s.content.as_ref(), head_budget - used);
            if !part.is_empty() {
                out.push(Span::styled(part, s.style));
            }
            break;
        }
    }
    out.push(Span::styled("…".to_string(), ellipsis_style));

    let mut tail: Vec<Span<'static>> = Vec::new();
    let mut used_t = 0;
    for s in spans.iter().rev() {
        if used_t >= tail_budget {
            break;
        }
        let w = display_width(s.content.as_ref());
        if used_t + w <= tail_budget {
            tail.push(owned(s));
            used_t += w;
        } else {
            let part = take_suffix(s.content.as_ref(), tail_budget - used_t);
            if !part.is_empty() {
                tail.push(Span::styled(part, s.style));
            }
            break;
        }
    }
    tail.reverse();
    out.extend(tail);
    out
}

/// 按显示宽度贪心换行（无空格的 CJK 句子按字符折行）。返回每行字符串。
/// `width` 为 0 时返回单行原文以避免死循环。
#[must_use]
pub fn wrap_by_width(s: &str, width: usize) -> Vec<String> {
    if width == 0 || display_width(s) <= width {
        return vec![s.to_string()];
    }
    let mut lines = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if cur_w + cw > width && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += cw;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipsize_middle_keeps_head_and_tail() {
        // 12 列预算裁 "/aaaa/bbbb/cccc"（15 宽）→ 保头保尾 + 中段 …
        let out = ellipsize_middle("/aaaa/bbbb/cccc", 12);
        assert!(display_width(&out) <= 12, "结果宽度不得超预算：{out:?}");
        assert!(out.contains('…'), "应含省略号");
        assert!(out.starts_with('/'), "应保留头部");
        assert!(out.ends_with("cccc") || out.ends_with("ccc"), "应保留尾部：{out}");
    }

    #[test]
    fn ellipsize_middle_noop_when_fits() {
        assert_eq!(ellipsize_middle("short", 10), "short");
    }

    #[test]
    fn ellipsize_middle_cjk_respects_display_width() {
        // 中文每字 2 宽；"缓存目录名称很长啊" = 9 字 = 18 宽，裁到 10 列
        let out = ellipsize_middle("缓存目录名称很长啊", 10);
        assert!(display_width(&out) <= 10, "CJK 裁剪不得超显示宽度：{out}={}", display_width(&out));
        assert!(out.contains('…'));
    }

    #[test]
    fn truncate_end_appends_ellipsis() {
        let out = truncate_end("abcdefghij", 5);
        assert_eq!(display_width(&out), 5);
        assert!(out.ends_with('…'));
        assert!(out.starts_with("abcd"));
    }

    #[test]
    fn wrap_by_width_splits_cjk() {
        // 8 宽度 → 每行至多 4 个中文字
        let lines = wrap_by_width("一二三四五六七", 8);
        assert!(lines.len() >= 2, "应折成多行：{lines:?}");
        for l in &lines {
            assert!(display_width(l) <= 8, "每行不超宽：{l}");
        }
        assert_eq!(lines.concat(), "一二三四五六七", "拼接应还原原文");
    }
}
