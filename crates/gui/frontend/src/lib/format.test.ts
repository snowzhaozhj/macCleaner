import { describe, it, expect } from "vitest";
import { formatBytes } from "./format";

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
