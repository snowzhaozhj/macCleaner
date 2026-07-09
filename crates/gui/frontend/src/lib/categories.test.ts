import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { KNOWN_CATEGORIES } from "./categories";

/**
 * Parity 守卫：前端预印分类集必须覆盖后端 clean_rules.toml 里出现的每个 category。
 * 否则新品类的行会在扫描中途插入 → 行新增 → 跳变（破坏防跳变本质目标）。
 */
describe("KNOWN_CATEGORIES 与 clean_rules.toml parity", () => {
  it("覆盖后端规则里所有 category", () => {
    const tomlPath = fileURLToPath(
      new URL("../../../../core/src/clean_rules.toml", import.meta.url),
    );
    const toml = readFileSync(tomlPath, "utf8");
    const backend = new Set(
      [...toml.matchAll(/category\s*=\s*"([^"]+)"/g)].map((m) => m[1]),
    );
    expect(backend.size).toBeGreaterThan(0); // 断言确实读到了规则
    const known = new Set<string>(KNOWN_CATEGORIES);
    const missing = [...backend].filter((c) => !known.has(c));
    expect(missing, `前端未预印的后端分类: ${missing.join(", ")}`).toEqual([]);
  });
});
