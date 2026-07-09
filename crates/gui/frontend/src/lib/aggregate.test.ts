import { describe, it, expect } from "vitest";
import {
  upsertFound,
  aggregateByCategory,
  computeSegments,
  summarizeReport,
  itemKey,
  type LiveItem,
  type FoundData,
} from "./format";
import type { CleanReport } from "./ipc";

function found(over: Partial<FoundData> = {}): FoundData {
  return {
    category: "系统缓存",
    path: "/tmp/a",
    size: 100,
    safety: "Safe",
    impact: "缓存",
    recovery: "会自动重建",
    preselect: true,
    ...over,
  };
}

describe("upsertFound（流式合并 delta）", () => {
  it("首次出现建项，预选 = 非 Risky 且 preselect", () => {
    const items: LiveItem[] = [];
    const index = new Map<string, number>();
    upsertFound(items, index, found());
    expect(items).toHaveLength(1);
    expect(items[0].size).toBe(100);
    expect(items[0].selected).toBe(true);
  });

  it("同一 (category,path) 多次 delta 累加，不产生重复行（edge, AE1）", () => {
    const items: LiveItem[] = [];
    const index = new Map<string, number>();
    upsertFound(items, index, found({ size: 100 }));
    upsertFound(items, index, found({ size: 50 }));
    upsertFound(items, index, found({ size: 25 }));
    expect(items).toHaveLength(1);
    expect(items[0].size).toBe(175);
  });

  it("同 path 不同 category 视为不同项", () => {
    const items: LiveItem[] = [];
    const index = new Map<string, number>();
    upsertFound(items, index, found({ category: "系统缓存" }));
    upsertFound(items, index, found({ category: "浏览器缓存" }));
    expect(items).toHaveLength(2);
  });

  it("Risky 永不预选（安全模型不变量）", () => {
    const items: LiveItem[] = [];
    const index = new Map<string, number>();
    upsertFound(items, index, found({ safety: "Risky", preselect: true }));
    expect(items[0].selected).toBe(false);
  });

  it("preselect=false 不预选", () => {
    const items: LiveItem[] = [];
    const index = new Map<string, number>();
    upsertFound(items, index, found({ preselect: false }));
    expect(items[0].selected).toBe(false);
  });

  it("itemKey 对含空格路径仍唯一区分 category 边界", () => {
    expect(itemKey("a", "b c")).not.toBe(itemKey("a b", "c"));
  });
});

describe("aggregateByCategory", () => {
  const items: LiveItem[] = [
    mkItem("系统缓存", "/s1", 300, true),
    mkItem("系统缓存", "/s2", 200, false),
    mkItem("浏览器缓存", "/b1", 100, true),
  ];

  it("按分类聚合体积/计数/已选量", () => {
    const aggs = aggregateByCategory(items);
    const sys = aggs.find((a) => a.name === "系统缓存")!;
    expect(sys.size).toBe(500);
    expect(sys.count).toBe(2);
    expect(sys.selectedSize).toBe(300);
    expect(sys.selectedCount).toBe(1);
  });

  it("扫描期保留 0 命中的已知分类（骨架占位，R2）", () => {
    const order = ["系统缓存", "浏览器缓存", "开发缓存"];
    const aggs = aggregateByCategory([], order);
    expect(aggs.map((a) => a.name)).toEqual(order);
    expect(aggs.every((a) => a.count === 0)).toBe(true);
  });

  it("dropEmpty=true 收拢 0 命中分类（R3 完成时一次性收拢）", () => {
    const order = ["系统缓存", "浏览器缓存", "开发缓存"];
    const aggs = aggregateByCategory(items, order, true);
    expect(aggs.map((a) => a.name)).toEqual(["系统缓存", "浏览器缓存"]);
  });

  it("已知分类顺序优先，未知分类追加在后（发现序，扫描期不重排）", () => {
    const withExtra = [...items, mkItem("其它", "/x", 10, true)];
    const aggs = aggregateByCategory(withExtra, ["系统缓存", "浏览器缓存"]);
    expect(aggs.map((a) => a.name)).toEqual(["系统缓存", "浏览器缓存", "其它"]);
  });
});

describe("computeSegments", () => {
  it("占比计算正确且总和为 1（U4 test scenario）", () => {
    const aggs = aggregateByCategory([
      mkItem("系统缓存", "/s", 750, true),
      mkItem("浏览器缓存", "/b", 250, true),
    ]);
    const segs = computeSegments(aggs);
    expect(segs.map((s) => s.fraction)).toEqual([0.75, 0.25]);
    expect(segs.reduce((s, x) => s + x.fraction, 0)).toBeCloseTo(1, 10);
  });

  it("保留 0 体积分类（图例行数稳定，防跳变），其 fraction=0", () => {
    const aggs = aggregateByCategory([mkItem("系统缓存", "/s", 100, true)], [
      "系统缓存",
      "浏览器缓存",
    ]);
    const segs = computeSegments(aggs);
    expect(segs.map((s) => s.name)).toEqual(["系统缓存", "浏览器缓存"]);
    expect(segs.find((s) => s.name === "浏览器缓存")!.fraction).toBe(0);
  });

  it("空分类列表返回空数组；全 0 体积时各段 fraction=0", () => {
    expect(computeSegments([])).toEqual([]);
    const zero = aggregateByCategory([], ["系统缓存", "浏览器缓存"]);
    expect(computeSegments(zero).every((s) => s.fraction === 0)).toBe(true);
  });
});

describe("summarizeReport", () => {
  const report: CleanReport = {
    cleaned: [
      { path: "/ok1", size: 100, success: true, error: null },
      { path: "/ok2", size: 200, success: true, error: null },
      { path: "/bad", size: 50, success: false, error: "权限不足" },
    ],
    total_freed: 300,
    success_count: 2,
    failure_count: 1,
  };

  it("成功/失败分列，freed 取后端 total_freed（U7）", () => {
    const r = summarizeReport(report);
    expect(r.freed).toBe(300);
    expect(r.successCount).toBe(2);
    expect(r.failureCount).toBe(1);
    expect(r.succeeded.map((e) => e.path)).toEqual(["/ok1", "/ok2"]);
    expect(r.failed).toEqual([{ path: "/bad", size: 50, error: "权限不足" }]);
  });

  it("全成功时失败列表为空", () => {
    const r = summarizeReport({
      cleaned: [{ path: "/ok", size: 10, success: true, error: null }],
      total_freed: 10,
      success_count: 1,
      failure_count: 0,
    });
    expect(r.failed).toEqual([]);
  });
});

function mkItem(
  category: string,
  path: string,
  size: number,
  selected: boolean,
): LiveItem {
  return {
    path,
    size,
    safety: "Safe",
    category,
    impact: "",
    recovery: "",
    selected,
  };
}
