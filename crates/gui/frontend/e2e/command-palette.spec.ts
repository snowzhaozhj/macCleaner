import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall } from "./support/tauri-mock";
import { defaultHandlers, fdaUnauthorized } from "./support/fixtures";

// U3 Cmd+K 命令面板 E2E（R1/R3/R4/R5/R7）。
// 加速器契约：键盘唤起 → 模糊匹配 → 键盘/鼠标执行 → 关闭并还原焦点；四 tab 可见导航不回归。

test("Cmd+K 在主界面打开面板、输入获焦；再次 Cmd+K 与 Esc 均关闭", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  const palette = page.getByRole("dialog", { name: "命令面板" });
  const input = page.getByPlaceholder("搜索命令…");

  await page.keyboard.press("ControlOrMeta+k");
  await expect(palette).toBeVisible();
  await expect(input).toBeFocused();

  // 再次 Cmd+K 关闭。
  await page.keyboard.press("ControlOrMeta+k");
  await expect(palette).toHaveCount(0);

  // 重新打开后 Esc 关闭。
  await page.keyboard.press("ControlOrMeta+k");
  await expect(palette).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(palette).toHaveCount(0);
});

test("onboarding 态按 Cmd+K 不打开面板（R1 边界：仅 ready 唤起）", async ({ page }) => {
  await installTauriMock(page, { ...defaultHandlers(), check_fda: { result: fdaUnauthorized() } });
  await page.goto("/");

  // 未授权 → 引导页；确认不在主界面。
  await expect(page.getByRole("dialog", { name: "命令面板" })).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByRole("dialog", { name: "命令面板" })).toHaveCount(0);
});

test("输入过滤 + Enter 执行导航命令 → 切到目标 tab、面板关闭（R4）", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  await page.keyboard.press("ControlOrMeta+k");
  await page.getByPlaceholder("搜索命令…").fill("uninstall");

  // 过滤到卸载命令且高亮首项。
  const option = page.getByRole("option", { name: "卸载" });
  await expect(option).toBeVisible();
  await expect(option).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("Enter");

  // 切到卸载 tab（statusbar 模式文案）+ 面板关闭。
  await expect(page.getByText("卸载模式")).toBeVisible();
  await expect(page.getByRole("dialog", { name: "命令面板" })).toHaveCount(0);
});

test("↑ 从首项环绕到末项；点击某项执行（R3）", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  await page.keyboard.press("ControlOrMeta+k");
  // 空 query：全部命令，首项「清理」高亮（exact：避免命中「开发清理」）。
  await expect(page.getByRole("option", { name: "清理", exact: true })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  // ↑ 环绕到末项「打开磁盘访问权限设置」。
  await page.keyboard.press("ArrowUp");
  await expect(page.getByRole("option", { name: "打开磁盘访问权限设置" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  // 鼠标点击「分析」直接执行 → 切到分析 tab。
  await page.getByRole("option", { name: "分析" }).click();
  await expect(page.getByText("分析模式")).toBeVisible();
});

test("点 backdrop 空白关闭（R5）", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByRole("dialog", { name: "命令面板" })).toBeVisible();

  // 面板居中，顶部区域是 backdrop 本体；点其左上角触发关闭。
  await page.locator(".backdrop").click({ position: { x: 5, y: 5 } });
  await expect(page.getByRole("dialog", { name: "命令面板" })).toHaveCount(0);
});

test("执行「打开废纸篓」命令调用 open_trash", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  await page.keyboard.press("ControlOrMeta+k");
  await page.getByPlaceholder("搜索命令…").fill("trash");
  await page.getByRole("option", { name: "打开废纸篓" }).click();

  expect(await lastCall(page, "open_trash")).not.toBeNull();
});

test("R7 回归：四 tab 可见导航仍在、可点（面板是加速器非替代）", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  const nav = page.getByRole("navigation", { name: "功能切换" });
  await expect(nav.getByRole("button", { name: "清理", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "开发清理", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "卸载", exact: true })).toBeVisible();
  await expect(nav.getByRole("button", { name: "分析", exact: true })).toBeVisible();

  // 点可见 tab 仍切换（不依赖面板）。
  await nav.getByRole("button", { name: "分析", exact: true }).click();
  await expect(page.getByText("分析模式")).toBeVisible();
});
