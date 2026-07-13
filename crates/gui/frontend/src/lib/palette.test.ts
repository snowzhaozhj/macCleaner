import { describe, it, expect } from "vitest";
import { fuzzyFilter, type Command } from "./palette";

// 构造一批测试命令（run 为 no-op，过滤只看 title/keywords）。
function cmd(id: string, title: string, keywords?: string[]): Command {
  return { id, title, keywords, run: () => {} };
}

const noop = () => {};

describe("fuzzyFilter", () => {
  it("空 query 返回全部命令，顺序不变", () => {
    const cmds = [cmd("a", "清理"), cmd("b", "分析"), cmd("c", "卸载")];
    const out = fuzzyFilter(cmds, "");
    expect(out.map((c) => c.id)).toEqual(["a", "b", "c"]);
  });

  it("空白 query 视同空，返回全部", () => {
    const cmds = [cmd("a", "清理"), cmd("b", "分析")];
    expect(fuzzyFilter(cmds, "   ").map((c) => c.id)).toEqual(["a", "b"]);
  });

  it("子序列命中 title", () => {
    const cmds = [cmd("clean", "clean"), cmd("analyze", "analyze")];
    const out = fuzzyFilter(cmds, "cl");
    expect(out.map((c) => c.id)).toContain("clean");
    expect(out.map((c) => c.id)).not.toContain("analyze");
  });

  it("大小写不敏感", () => {
    const cmds = [cmd("clean", "clean")];
    expect(fuzzyFilter(cmds, "CLEAN").map((c) => c.id)).toEqual(["clean"]);
    expect(fuzzyFilter(cmds, "Clean").map((c) => c.id)).toEqual(["clean"]);
  });

  it("keyword 命中（title 为中文，靠 keyword 拼音/英文别名）", () => {
    const cmds = [cmd("clean", "清理", ["clean", "qingli"]), cmd("analyze", "分析", ["analyze"])];
    const out = fuzzyFilter(cmds, "qingli");
    expect(out.map((c) => c.id)).toEqual(["clean"]);
  });

  it("连续 + 词首命中的分高于零散子序列命中", () => {
    // "clean" 对含 keyword "clean" 的命令是连续词首满命中；
    // "cancel"（c…l 零散）只是弱子序列命中——clean 应排在前。
    const cmds = [
      cmd("cancel", "取消扫描", ["cancel scan"]),
      cmd("clean", "清理", ["clean"]),
    ];
    const out = fuzzyFilter(cmds, "clean");
    expect(out[0].id).toBe("clean");
  });

  it("无匹配返回空数组", () => {
    const cmds = [cmd("clean", "清理", ["clean"]), cmd("analyze", "分析", ["analyze"])];
    expect(fuzzyFilter(cmds, "zzzz")).toEqual([]);
  });

  it("同分命令保持输入相对顺序（稳定排序）", () => {
    // 两条命令对 "a" 的命中位置相同（title 第 2 位、非词首非连续），分相等。
    const cmds: Command[] = [
      { id: "x", title: "xa", run: noop },
      { id: "y", title: "ya", run: noop },
    ];
    const out = fuzzyFilter(cmds, "a");
    expect(out.map((c) => c.id)).toEqual(["x", "y"]);
  });
});
