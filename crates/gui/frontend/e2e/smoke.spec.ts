import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall } from "./support/tauri-mock";
import { defaultHandlers, scanStream, scanResult, scanItem } from "./support/fixtures";

// U1 骨架自测：证明 mock 注入通路成立——app 能经 mock invoke 启动、Channel 事件流能回放到 UI、
// invoke 调用被记录。这是整套 E2E 的地基，任何流程 spec 出问题先看它是否绿。

test("app 经 mock 启动并到达清理界面（invoke check_fda 被记录）", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  // check_fda authorized → 跳过 Onboarding，直达 Clean 首屏摘要。
  await expect(page.getByText("可安全释放")).toBeVisible();
  expect(await lastCall(page, "check_fda")).not.toBeNull();
});

test("Channel 事件流回放：scan_clean 的 Found→结果渲染到分类行", async ({ page }) => {
  const items = [
    scanItem("/Library/Caches/a", 5 * 1024 * 1024, { category: "系统缓存" }),
    scanItem("/Library/Caches/b", 3 * 1024 * 1024, { category: "系统缓存" }),
  ];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
  });
  await page.goto("/");

  // 权威终值来自 resolved ScanResult：系统缓存分类应显示 2 项。
  await expect(page.getByRole("button", { name: /系统缓存/ })).toBeVisible();
  await expect(page.getByText("2 项")).toBeVisible();
  // scan_clean 被调用且带了 onEvent 通道（脱敏为 [Channel]）。
  const call = await lastCall(page, "scan_clean");
  expect(call?.args.onEvent).toBe("[Channel]");
});
