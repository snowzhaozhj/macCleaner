import { describe, it, expect } from "vitest";
import { nextToast, dismissToast, type ToastState } from "./toast";

describe("toast 单实例（U5 / R11）", () => {
  it("首个 toast：seq 从 1 起", () => {
    const t = nextToast(null, 3, 1024);
    expect(t).toEqual({ count: 3, freed: 1024, seq: 1 });
  });

  it("连续删除替换为单个对象、不堆叠，seq 递增", () => {
    let s: ToastState = null;
    s = nextToast(s, 2, 100);
    s = nextToast(s, 5, 200);
    s = nextToast(s, 1, 50);
    // 始终是单个对象（非数组），只保留最新一次内容
    expect(s).toEqual({ count: 1, freed: 50, seq: 3 });
  });

  it("dismiss 清空为 null", () => {
    const s = nextToast(null, 1, 10);
    expect(s?.seq).toBe(1);
    expect(dismissToast()).toBeNull();
    // dismiss 后再来一次，seq 从头（无前值）
    expect(nextToast(dismissToast(), 2, 20)).toMatchObject({ seq: 1, count: 2 });
  });
});
