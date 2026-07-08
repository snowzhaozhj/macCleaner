import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall } from "./support/tauri-mock";
import { defaultHandlers, fdaAuthorized, fdaUnauthorized } from "./support/fixtures";

// U6 Onboarding / FDA 权限门 E2E（R3）。三态：authorized 直达主界面；unauthorized 显示引导 +
// 跳设置；check_fda 抛错则降级进主界面（不卡在 checking）。

test("authorized：跳过引导，直达清理主界面", async ({ page }) => {
  await installTauriMock(page, { ...defaultHandlers(), check_fda: { result: fdaAuthorized() } });
  await page.goto("/");
  await expect(page.getByText("可安全释放")).toBeVisible();
  await expect(page.getByRole("heading", { name: "需要完全磁盘访问权限" })).toHaveCount(0);
});

test("unauthorized：显示引导与探测，点『打开系统设置』触发 open_fda_settings", async ({ page }) => {
  await installTauriMock(page, { ...defaultHandlers(), check_fda: { result: fdaUnauthorized() } });
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "需要完全磁盘访问权限" })).toBeVisible();
  // 探测行呈现（fdaUnauthorized 提供两条）。exact 避免 "/Library/Caches" 命中 "~/Library/Caches"。
  await expect(page.getByText("/Library/Caches", { exact: true })).toBeVisible();
  await expect(page.getByText("无权限")).toBeVisible();

  await page.getByRole("button", { name: "打开系统设置" }).click();
  expect(await lastCall(page, "open_fda_settings")).not.toBeNull();
});

test("check_fda 抛错：降级进主界面，不卡在检查态", async ({ page }) => {
  await installTauriMock(page, { ...defaultHandlers(), check_fda: { error: "TCC 不可用" } });
  await page.goto("/");

  // 降级：进入主界面（Clean 自动扫描），不停留在「检查磁盘访问权限…」。
  await expect(page.getByText("可安全释放")).toBeVisible();
  await expect(page.getByText("检查磁盘访问权限…")).toHaveCount(0);
});
