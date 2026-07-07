/**
 * 安全等级 → 三通道编码（DESIGN.md §1.1 / §8）。
 * 不变量：色 + 字形 + 文字恒同现，永不退化为纯色块。
 * glyph 独占非三角家族 ● ◆ ✕（与导航展开符 ▶ ▼ 分属两轴）。
 */
import type { SafetyLevel } from "./ipc";

export type SafetyDescriptor = {
  glyph: string; // ● / ◆ / ✕
  label: string; // 安全 / 中等 / 危险
  tokenVar: string; // CSS 变量名（含 var() 包裹）
};

const MAP: Record<SafetyLevel, SafetyDescriptor> = {
  Safe: { glyph: "●", label: "安全", tokenVar: "var(--safety-safe)" },
  Moderate: { glyph: "◆", label: "中等", tokenVar: "var(--safety-moderate)" },
  Risky: { glyph: "✕", label: "危险", tokenVar: "var(--safety-risky)" },
};

export function safetyDescriptor(level: SafetyLevel): SafetyDescriptor {
  return MAP[level];
}
