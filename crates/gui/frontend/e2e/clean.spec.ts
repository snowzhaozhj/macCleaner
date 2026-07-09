import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall, callsFor } from "./support/tauri-mock";
import {
  defaultHandlers,
  scanStream,
  scanResult,
  scanItem,
  cleanStream,
  cleanReport,
} from "./support/fixtures";

// U4 Clean 流 E2E（R3 / SC3 / SC5）。主干：扫描→填充→（勾选）→删除→回执→撤销；
// 关键分支：Risky type-to-confirm、中途取消、Error 事件。每个交互断言 invoke 命令名 + 参数 + UI 响应。

const MB = 1024 * 1024;

test("扫描填充：ScanResult 渲染分类行与首屏可释放量", async ({ page }) => {
  const items = [
    scanItem("/Library/Caches/a", 5 * MB, { category: "系统缓存" }),
    scanItem("/Library/Caches/b", 3 * MB, { category: "系统缓存" }),
  ];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
  });
  await page.goto("/");

  await expect(page.getByRole("button", { name: /系统缓存/ })).toBeVisible();
  await expect(page.getByText("2 项")).toBeVisible();
  // 首屏「可安全释放」= 已选（Safe 预选）合计 8 MiB。
  await expect(page.getByText("8.00 MiB").first()).toBeVisible();
});

test("纯 Safe 删除：确认弹窗不出现，clean 以空口令触发，回执+撤销吐司呈现", async ({ page }) => {
  const items = [
    scanItem("/Library/Caches/a", 5 * MB),
    scanItem("/Library/Caches/b", 3 * MB),
  ];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: { events: cleanStream(paths, 8 * MB), result: cleanReport(paths, 4 * MB) },
  });
  await page.goto("/");

  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 全 Safe：不弹 type-to-confirm 模态，直删。
  await expect(page.getByRole("dialog")).toHaveCount(0);

  const call = await lastCall(page, "clean");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe(""); // 非 Risky：空口令
  expect(call?.args.onEvent).toBe("[Channel]");

  // 完成：回执 + 撤销吐司。
  await expect(page.getByText(/已释放/)).toBeVisible();
  await expect(page.getByRole("status")).toContainText("已移到废纸篓");
});

test("撤销：点『在访达中恢复』触发 open_trash", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: { events: cleanStream(paths, 5 * MB), result: cleanReport(paths, 5 * MB) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  await page.getByRole("button", { name: "在访达中恢复" }).first().click();
  expect(await lastCall(page, "open_trash")).not.toBeNull();
});

test("含 Risky：必须 type-to-confirm，输入 delete 前删除按钮禁用，确认后 clean 带口令 delete", async ({ page }) => {
  const safe = scanItem("/Library/Caches/a", 5 * MB, { category: "系统缓存" });
  const risky = scanItem("/Library/Caches/danger", 9 * MB, {
    category: "系统缓存",
    safety: "Risky",
    selected: false,
  });
  const items = [safe, risky];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: {
      events: cleanStream([safe.path, risky.path], 14 * MB),
      result: cleanReport([safe.path, risky.path], 7 * MB),
    },
  });
  await page.goto("/");

  // 展开分类，勾选 Risky 项（Risky 永不预选，须手动选中才进入删除集）。
  await page.getByRole("button", { name: /系统缓存/ }).click();
  await page.getByRole("checkbox", { name: risky.path }).check();

  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 弹出确认模态，删除按钮初始禁用。
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const deleteBtn = dialog.getByRole("button", { name: "删除" });
  await expect(deleteBtn).toBeDisabled();

  // 错口令仍禁用。
  await dialog.getByRole("textbox").fill("nope");
  await expect(deleteBtn).toBeDisabled();

  // 正确口令 delete → 启用 → 确认。
  await dialog.getByRole("textbox").fill("delete");
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  const call = await lastCall(page, "clean");
  expect(call?.args.confirmToken).toBe("delete");
  expect(call?.args.paths).toEqual([safe.path, risky.path]);
});

test("中途取消：扫描态可见取消按钮，点击触发 cancel_scan 并回到结果态", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    // pending：scan_clean 悬挂不 resolve，扫描态保持，直到 cancel_scan 到来 reject。
    scan_clean: { events: scanStream([]), result: scanResult([]), pending: true },
  });
  await page.goto("/");

  const cancelBtn = page.getByRole("button", { name: "取消" });
  await expect(cancelBtn).toBeVisible();
  await cancelBtn.click();

  expect(await lastCall(page, "cancel_scan")).not.toBeNull();
  // 取消后离开扫描态：重新扫描按钮出现。
  await expect(page.getByRole("button", { name: /重新扫描|再次扫描/ })).toBeVisible();
});

test("Error 事件：结果态显示错误横幅而非静默", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream([], { error: "扫描失败：磁盘不可读" }), result: scanResult([]) },
  });
  await page.goto("/");

  await expect(page.getByRole("alert")).toContainText("扫描失败：磁盘不可读");
});

test("每个删除都携带 onEvent 通道（流式回执前置）", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: { events: cleanStream(paths, 5 * MB), result: cleanReport(paths, 5 * MB) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const calls = await callsFor(page, "clean");
  expect(calls.length).toBe(1);
  expect(calls[0].args.onEvent).toBe("[Channel]");
});
