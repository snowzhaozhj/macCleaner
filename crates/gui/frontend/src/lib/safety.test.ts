import { describe, it, expect } from "vitest";
import { safetyDescriptor } from "./safety";

describe("safetyDescriptor", () => {
  it("Safe → 圆 + 安全 + safe token", () => {
    const d = safetyDescriptor("Safe");
    expect(d.glyph).toBe("●");
    expect(d.label).toBe("安全");
    expect(d.tokenVar).toBe("var(--safety-safe)");
  });

  it("Moderate → 菱 + 中等 + moderate token", () => {
    const d = safetyDescriptor("Moderate");
    expect(d.glyph).toBe("◆");
    expect(d.label).toBe("中等");
    expect(d.tokenVar).toBe("var(--safety-moderate)");
  });

  it("Risky → 叉 + 危险 + risky token", () => {
    const d = safetyDescriptor("Risky");
    expect(d.glyph).toBe("✕");
    expect(d.label).toBe("危险");
    expect(d.tokenVar).toBe("var(--safety-risky)");
  });

  it("字形独占非三角家族，不与导航 ▶▼ 撞", () => {
    const glyphs = (["Safe", "Moderate", "Risky"] as const).map(
      (l) => safetyDescriptor(l).glyph,
    );
    expect(glyphs).toEqual(["●", "◆", "✕"]);
    expect(glyphs).not.toContain("▶");
    expect(glyphs).not.toContain("▼");
  });
});
