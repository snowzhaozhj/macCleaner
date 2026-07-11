import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { KNOWN_CATEGORIES, KNOWN_PURGE_CATEGORIES } from "./categories";

/** 从规则 TOML 源码提取 category 集合（clean/purge 两张表共用）。 */
function backendCategories(relTomlPath: string): Set<string> {
  const tomlPath = fileURLToPath(new URL(relTomlPath, import.meta.url));
  const toml = readFileSync(tomlPath, "utf8");
  return new Set([...toml.matchAll(/category\s*=\s*"([^"]+)"/g)].map((m) => m[1]));
}

/**
 * Parity 守卫：前端预印分类集必须覆盖后端 clean_rules.toml 里出现的每个 category。
 * 否则新品类的行会在扫描中途插入 → 行新增 → 跳变（破坏防跳变本质目标）。
 */
describe("KNOWN_CATEGORIES 与 clean_rules.toml parity", () => {
  it("覆盖后端规则里所有 category", () => {
    const backend = backendCategories("../../../../core/src/clean_rules.toml");
    expect(backend.size).toBeGreaterThan(0); // 断言确实读到了规则
    const known = new Set<string>(KNOWN_CATEGORIES);
    const missing = [...backend].filter((c) => !known.has(c));
    expect(missing, `前端未预印的后端分类: ${missing.join(", ")}`).toEqual([]);
  });
});

/**
 * Purge parity 比 clean 更严格：断言**集合相等**而非仅覆盖——purge 预印 13 个占位行，
 * 前端多出的幽灵分类会永远占一行空位（clean 只有 2 类无此风险，维持原覆盖断言）。
 */
describe("KNOWN_PURGE_CATEGORIES 与 purge_rules.toml parity", () => {
  it("与后端规则的 category 集合完全相等", () => {
    const backend = backendCategories("../../../../core/src/purge_rules.toml");
    expect(backend.size).toBeGreaterThan(0);
    expect(new Set<string>(KNOWN_PURGE_CATEGORIES)).toEqual(backend);
  });
});
