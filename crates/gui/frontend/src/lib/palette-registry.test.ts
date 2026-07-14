import { describe, it, expect, afterEach } from "vitest";
import type { Command } from "./palette";
import { routeCommands, setRouteCommands, clearRouteCommands } from "./palette-registry.svelte";

// 注册表纯机制单测（U5）：set/clear/覆盖语义与 id 无碰撞——单路由不变量的机械保证。
// 反应式合并（静态 + 路由）与随挂载/相位增删由 e2e（真 Svelte 运行时）覆盖。

const cmd = (id: string): Command => ({ id, title: id, run: () => {} });

afterEach(() => clearRouteCommands());

describe("palette-registry", () => {
  it("默认空", () => {
    expect(routeCommands()).toEqual([]);
  });

  it("setRouteCommands 写入当前路由命令集", () => {
    const a = cmd("clean.scan");
    const b = cmd("clean.trash");
    setRouteCommands([a, b]);
    expect(routeCommands()).toEqual([a, b]);
  });

  it("覆盖式写入（非追加）——第二次调用替换而非累加", () => {
    setRouteCommands([cmd("a1")]);
    setRouteCommands([cmd("b1"), cmd("b2")]);
    expect(routeCommands().map((c) => c.id)).toEqual(["b1", "b2"]);
  });

  it("clearRouteCommands 置空（模拟路由卸载）", () => {
    setRouteCommands([cmd("x")]);
    clearRouteCommands();
    expect(routeCommands()).toEqual([]);
  });

  it("路由命令 id 均带路由命名空间前缀，与静态 nav./act. 无碰撞（KTD4）", () => {
    // 各路由实际使用的命令 id（与四路由源保持一致）。
    const routeIds = [
      "clean.scan", "clean.cancel", "clean.trash",
      "purge.chooseDir", "purge.scan", "purge.cancel", "purge.trash",
      "uninstall.rescan", "uninstall.back", "uninstall.trash",
      "analyze.start", "analyze.cancel", "analyze.deleteMarked",
    ];
    const staticIds = ["nav.clean", "nav.purge", "nav.uninstall", "nav.analyze", "act.trash", "act.fda"];
    for (const id of routeIds) {
      expect(staticIds).not.toContain(id);
      expect(id).toMatch(/^(clean|purge|uninstall|analyze)\./);
    }
    // 全集无重复。
    const all = [...staticIds, ...routeIds];
    expect(new Set(all).size).toBe(all.length);
  });
});
