import { describe, it, expect } from "vitest";
import { CONFIRM_TOKEN, isConfirmed } from "./confirm";

describe("confirm", () => {
  it("token 为 delete", () => {
    expect(CONFIRM_TOKEN).toBe("delete");
  });

  it("精确匹配为真", () => {
    expect(isConfirmed("delete")).toBe(true);
  });

  it("大小写不敏感", () => {
    expect(isConfirmed("DELETE")).toBe(true);
    expect(isConfirmed("Delete")).toBe(true);
  });

  it("去首尾空格", () => {
    expect(isConfirmed(" delete ")).toBe(true);
    expect(isConfirmed("\tdelete\n")).toBe(true);
  });

  it("非精确匹配为假", () => {
    expect(isConfirmed("delet")).toBe(false);
    expect(isConfirmed("")).toBe(false);
    expect(isConfirmed("deletee")).toBe(false);
    expect(isConfirmed("de lete")).toBe(false);
  });
});
