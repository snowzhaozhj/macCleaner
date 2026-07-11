import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it, expect } from "vitest";
import { analyzeCommand, shellQuote, formatBytes, dirSegments } from "./format";

function parseWithZsh(command: string, env: NodeJS.ProcessEnv = process.env): string[] {
  const script = `mc() {
    printf '%s\\0' "$#"
    printf '%s\\0' "$@"
  }
  ${command}`;
  const output = execFileSync("/bin/zsh", ["-c", script], { env });
  return output.toString().split("\0").slice(0, -1);
}

describe("analyzeCommand", () => {
  it.each([
    ["普通绝对路径", "/Users/test/Library/Caches"],
    ["空格", "/Users/test/Application Support/cache"],
    ["单引号", "/Users/test/a'b"],
    ["换行", "/Users/test/line\nbreak"],
    ["反斜杠", String.raw`/Users/test/a\b`],
    ["Unicode", "/Users/测试/缓存📦"],
    ["分号", "/Users/test/cache;echo injected"],
    ["命令替换", "/Users/test/$(echo injected)"],
    ["反引号", "/Users/test/`echo injected`"],
  ])("zsh 解析%s后，路径仍是原始的单一参数", (_label, path) => {
    expect(parseWithZsh(analyzeCommand(path))).toEqual(["2", "analyze", path]);
  });

  it("不会执行路径中的命令替换或反引号", () => {
    const dir = mkdtempSync(join(tmpdir(), "maccleaner-shell-quote-"));
    const marker = join(dir, "executed");
    const path = '/tmp/$(touch "$MC_SENTINEL")/`touch "$MC_SENTINEL"`';

    try {
      expect(
        parseWithZsh(analyzeCommand(path), {
          ...process.env,
          MC_SENTINEL: marker,
        }),
      ).toEqual(["2", "analyze", path]);
      expect(existsSync(marker)).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

// Purge 的「命令行等价」按此形态拼接（Purge.svelte 的 `mc purge ${shellQuote(target)}`）；
// 含空格/元字符的目标目录必须仍作为单一参数到达 CLI（评审 R4：不给用户必然失败的命令）。
describe("shellQuote（Purge 命令行等价拼接形态）", () => {
  it.each([
    ["空格", "/Users/test/My Project"],
    ["单引号", "/Users/test/a'b"],
    ["命令替换", "/Users/test/$(echo injected)"],
  ])("zsh 解析%s路径后仍是原始的单一参数", (_label, path) => {
    expect(parseWithZsh(`mc purge ${shellQuote(path)}`)).toEqual(["2", "purge", path]);
  });
});

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
