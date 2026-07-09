import { describe, it, expect } from "vitest";
import { formatBytes, dirSegments } from "./format";

describe("formatBytes", () => {
  it("小于 1024 字节按 B 原样显示（无小数）", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1)).toBe("1 B");
    expect(formatBytes(1023)).toBe("1023 B");
  });

  it("按二进制单位逐级进位（KiB/MiB/GiB/TiB）", () => {
    expect(formatBytes(1024)).toBe("1.00 KiB");
    expect(formatBytes(1024 * 1024)).toBe("1.00 MiB");
    expect(formatBytes(1024 ** 3)).toBe("1.00 GiB");
    expect(formatBytes(1024 ** 4)).toBe("1.00 TiB");
  });

  it("最大单位封顶在 TiB，不再进位", () => {
    expect(formatBytes(1024 ** 5)).toBe("1024 TiB"); // 值封顶 1024，>=100 → 0 位小数
  });

  it("有效数字随量级收窄：>=100→0 位，>=10→1 位，否则 2 位", () => {
    expect(formatBytes(1024 * 5)).toBe("5.00 KiB"); // 5 → 2 位
    expect(formatBytes(1024 * 10)).toBe("10.0 KiB"); // 10 → 1 位
    expect(formatBytes(1024 * 100)).toBe("100 KiB"); // 100 → 0 位
    expect(formatBytes(1024 * 500)).toBe("500 KiB");
  });

  it("四舍五入符合 toFixed 语义", () => {
    // 1536 B = 1.5 KiB → 2 位小数
    expect(formatBytes(1536)).toBe("1.50 KiB");
  });
});

describe("dirSegments（move 5 空间地理分区）", () => {
  it("按体积降序取段，fraction 为占当前层总量之比", () => {
    const segs = dirSegments([
      { name: "a", size: 100 },
      { name: "b", size: 300 },
      { name: "c", size: 100 },
    ]);
    expect(segs.map((s) => s.name)).toEqual(["b", "a", "c"]);
    expect(segs[0].fraction).toBeCloseTo(0.6);
    expect(segs.reduce((s, x) => s + x.fraction, 0)).toBeCloseTo(1);
  });

  it("超过 topN 的子项合并为单个「其他」段（段数有界）", () => {
    const children = Array.from({ length: 8 }, (_, i) => ({
      name: `d${i}`,
      size: (8 - i) * 10, // d0=80 … d7=10，已降序
    }));
    const segs = dirSegments(children, 3);
    expect(segs).toHaveLength(4); // top3 + 其他
    expect(segs[3].name).toBe("其他");
    // 其他 = d3..d7 = 50+40+30+20+10 = 150
    expect(segs[3].size).toBe(150);
    expect(segs.reduce((s, x) => s + x.fraction, 0)).toBeCloseTo(1);
  });

  it("恰好等于 topN 时不产生「其他」段", () => {
    const segs = dirSegments(
      [
        { name: "a", size: 10 },
        { name: "b", size: 20 },
      ],
      2,
    );
    expect(segs).toHaveLength(2);
    expect(segs.some((s) => s.name === "其他")).toBe(false);
  });

  it("空目录 / 全 0 体积：无段或 fraction 全 0，不除零", () => {
    expect(dirSegments([])).toEqual([]);
    const zero = dirSegments([{ name: "a", size: 0 }]);
    expect(zero[0].fraction).toBe(0);
  });
});
