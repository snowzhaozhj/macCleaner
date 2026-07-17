import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, lastCall, callsFor } from "./support/tauri-mock";
import {
  defaultHandlers,
  appInfo,
  scanItem,
  scanResult,
  cleanStream,
  cleanReport,
  type HandlerSpec,
} from "./support/fixtures";
import type { AppInfo, ScanItem } from "../src/lib/ipc";

// move 7 第二段 Uninstall E2E（plan 021 / AE1–AE10）。两阶段：列应用→选应用解析残留→删除信任链。
// 关键分支：搜索过滤/零命中、扫描失败、残留预选与降级、纯 Safe 直删、删后剪树、隔离、布局。

const MB = 1024 * 1024;

/** 进入「卸载」tab（四 tab 导航，U5）。 */
async function gotoUninstall(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "卸载", exact: true }).click();
}

const SAFARI = appInfo("Safari", 300 * MB, {
  path: "/Applications/Safari.app",
  bundle_id: "com.apple.Safari",
});
const NOTES = appInfo("Notes", 80 * MB, {
  path: "/Applications/Notes.app",
  bundle_id: "com.apple.Notes",
});

/** 一条 app + 残留合成的 resolve_leftovers 结果（bundle 在前，残留在后）。 */
function leftoversFor(app: AppInfo, opts: { safeLeftover?: number; userData?: number } = {}): ScanItem[] {
  const items: ScanItem[] = [
    scanItem(app.path, app.size, { category: "应用" }),
  ];
  if (opts.safeLeftover) {
    items.push(scanItem(`~/Library/Caches/${app.bundle_id}`, opts.safeLeftover, { category: "应用残留 (Caches)" }));
  }
  if (opts.userData) {
    // 用户数据残留：Moderate + 不预选 + 证据（对齐 find_leftovers 的 D3 语义）。
    items.push(
      scanItem(`~/Library/Application Support/${app.bundle_id}`, opts.userData, {
        category: "应用残留 (Application Support)",
        safety: "Moderate",
        selected: false,
        impact: "可能含应用数据（数据库、缓存的文档/草稿、存档等）",
        recovery: "默认移入废纸篓可找回",
      }),
    );
  }
  return items;
}

function uninstallHandlers(apps: AppInfo[]): Record<string, HandlerSpec> {
  return { ...defaultHandlers(), scan_uninstall: { result: apps } };
}

test("AE1 进入卸载 tab：scan_uninstall 触发，应用按体积降序展示", async ({ page }) => {
  await installTauriMock(page, uninstallHandlers([NOTES, SAFARI]));
  await gotoUninstall(page);

  expect(await lastCall(page, "scan_uninstall")).not.toBeNull();
  await expect(page.getByRole("button", { name: /Safari/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Notes/ })).toBeVisible();
  await expect(page.getByText("2 个")).toBeVisible();
  // 体积降序：Safari(300) 行在 Notes(80) 行之前。
  const rows = page.locator("ul.apps .app-name");
  await expect(rows.nth(0)).toHaveText("Safari");
  await expect(rows.nth(1)).toHaveText("Notes");
});

test("AE2 搜索过滤：按名称大小写不敏感命中，清空恢复；零命中显示无匹配空态", async ({ page }) => {
  await installTauriMock(page, uninstallHandlers([SAFARI, NOTES]));
  await gotoUninstall(page);

  await page.getByRole("searchbox", { name: "搜索应用" }).fill("SAFARI");
  await expect(page.getByRole("button", { name: /Safari/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Notes/ })).toHaveCount(0);

  await page.getByRole("searchbox", { name: "搜索应用" }).fill("xyzzy");
  await expect(page.getByText(/没有匹配「xyzzy」的应用/)).toBeVisible();

  await page.getByRole("searchbox", { name: "搜索应用" }).fill("");
  await expect(page.getByRole("button", { name: /Notes/ })).toBeVisible();
});

test("AE8 扫描失败：listError 显示错误横幅、UI 不冻结、可重扫", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_uninstall: { error: "无法读取应用目录：/Applications: 权限不足" },
  });
  await gotoUninstall(page);

  await expect(page.getByRole("alert")).toContainText("权限不足");
  await expect(page.getByRole("button", { name: "重新扫描应用" })).toBeVisible();
});

test("AE3 选应用解析残留：resolve_leftovers 带 bundle_id，用户数据残留不预选", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB, userData: 30 * MB });
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { result: scanResult(items) },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();

  const call = await lastCall(page, "resolve_leftovers");
  expect(call?.args.appPath).toBe(SAFARI.path);
  expect(call?.args.bundleId).toBe(SAFARI.bundle_id);

  // 残留类目可见；预选合计 = 应用本体 300 + Caches 50 = 350 MiB（Application Support 30 未预选）。
  await expect(page.getByRole("button", { name: /应用残留 \(Application Support\)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /移入废纸篓 · 释放 350 MiB/ })).toBeVisible();
});

test("AE4 无 bundle_id：仅 app 本体，明示「未能解析残留」", async ({ page }) => {
  const noBundle = appInfo("Legacy", 40 * MB, { path: "/Applications/Legacy.app", bundle_id: null });
  await installTauriMock(page, {
    ...uninstallHandlers([noBundle]),
    resolve_leftovers: { result: scanResult(leftoversFor(noBundle)) },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Legacy/ }).click();

  const call = await lastCall(page, "resolve_leftovers");
  expect(call?.args.bundleId).toBeNull();
  await expect(page.getByText(/未能解析残留/)).toBeVisible();
});

test("AE10 有 bundle_id 但零残留：明示「未发现残留」（措辞区别于未能解析）", async ({ page }) => {
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { result: scanResult(leftoversFor(SAFARI)) }, // 仅 app 本体，无残留
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();

  await expect(page.getByText(/未发现残留，仅移除应用本体/)).toBeVisible();
  await expect(page.getByText(/未能解析残留/)).toHaveCount(0);
});

test("reviewError：resolve_leftovers 失败显示错误并可返回列表", async ({ page }) => {
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { error: "解析残留失败" },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();

  await expect(page.getByRole("alert")).toContainText("解析残留失败");
  await expect(page.getByRole("button", { name: "返回列表" })).toBeVisible();
});

test("AE5/AE7 纯 Safe 删除：无模态，uninstall 空口令触发，回执+撤销吐司", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  const paths = [SAFARI.path, `~/Library/Caches/${SAFARI.bundle_id}`];
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { result: scanResult(items) },
    uninstall: { events: cleanStream(paths, 350 * MB), result: cleanReport(paths, 175 * MB) },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  await expect(page.getByRole("dialog")).toHaveCount(0);
  const call = await lastCall(page, "uninstall");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe("");
  await expect(page.getByText(/已释放/)).toBeVisible();
  await expect(page.getByRole("status")).toContainText("已移到废纸篓");

  // KTD5：uninstall 回执不做真 undo——恢复入口仍是「在访达中恢复」（开 Finder），无「撤销清理」。
  await expect(page.getByRole("button", { name: /撤销清理/ })).toHaveCount(0);
  await page.getByRole("button", { name: "在访达中恢复" }).first().click();
  expect(await lastCall(page, "open_trash")).not.toBeNull();
  expect(await callsFor(page, "undo")).toEqual([]);
});

test("AE9 删后剪树：返回列表后已卸载应用被剔除", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  const paths = [SAFARI.path, `~/Library/Caches/${SAFARI.bundle_id}`];
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI, NOTES]),
    resolve_leftovers: { result: scanResult(items) },
    uninstall: { events: cleanStream(paths, 350 * MB), result: cleanReport(paths, 175 * MB) },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();
  await page.getByRole("button", { name: "返回应用列表" }).click();

  // Safari 已卸载被剔除；Notes 仍在。
  await expect(page.getByRole("button", { name: /Notes/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Safari/ })).toHaveCount(0);
});

test("AE6 隔离（前端面）：uninstall 只带残留审查结果的路径，不触发 clean/purge", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  const paths = [SAFARI.path, `~/Library/Caches/${SAFARI.bundle_id}`];
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { result: scanResult(items) },
    uninstall: { events: cleanStream(paths, 350 * MB), result: cleanReport(paths, 175 * MB) },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const call = await lastCall(page, "uninstall");
  expect(call?.args.paths).toEqual(paths);
  expect(await callsFor(page, "clean")).toEqual([]);
  expect(await callsFor(page, "purge")).toEqual([]);
});

test("删除 reject：done 相位诚实报错、不残留上次成功回执，应用不被剔除", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI, NOTES]),
    resolve_leftovers: { result: scanResult(items) },
    uninstall: { error: "删除失败：目标被占用" },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // done 相位显示失败横幅，不显示「已释放」成功回执。
  await expect(page.getByRole("alert")).toContainText("卸载失败");
  await expect(page.getByText(/已释放/)).toHaveCount(0);

  // 返回列表：Safari 未被剔除（删除失败，仍在装）。
  await page.getByRole("button", { name: "返回应用列表" }).click();
  await expect(page.getByRole("button", { name: /Safari/ })).toBeVisible();
});

test("部分失败：本体删除失败但残留成功时应用不被剔除（prune 只认本体删除）", async ({ page }) => {
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  const cachePath = `~/Library/Caches/${SAFARI.bundle_id}`;
  const paths = [SAFARI.path, cachePath];
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI, NOTES]),
    resolve_leftovers: { result: scanResult(items) },
    // 本体 .app 删除失败，仅缓存成功：success_count>0 但本体仍在。
    uninstall: {
      events: cleanStream([cachePath], 50 * MB, [cachePath]),
      result: cleanReport(paths, 50 * MB, { fail: [SAFARI.path] }),
    },
  });
  await gotoUninstall(page);
  await page.getByRole("button", { name: /Safari/ }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();
  await page.getByRole("button", { name: "返回应用列表" }).click();

  // 本体未删成功 → Safari 仍在列表（不能因删了缓存就误藏未卸载的应用）。
  await expect(page.getByRole("button", { name: /Safari/ })).toBeVisible();
});

test("四 tab 导航：卸载 tab 可切换，进入应用列表态，不串扰其它 tab", async ({ page }) => {
  await installTauriMock(page, uninstallHandlers([SAFARI]));
  await page.goto("/");

  await expect(page.getByRole("button", { name: "清理", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "开发清理" })).toBeVisible();
  await expect(page.getByRole("button", { name: "卸载", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "分析" })).toBeVisible();

  await page.getByRole("button", { name: "卸载", exact: true }).click();
  await expect(page.getByText("卸载模式")).toBeVisible();
  await expect(page.getByRole("button", { name: /Safari/ })).toBeVisible();
});

test("720×520 最小窗口：四 tab 与卸载操作可见可点、无横向滚动", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 520 });
  const items = leftoversFor(SAFARI, { safeLeftover: 50 * MB });
  await installTauriMock(page, {
    ...uninstallHandlers([SAFARI]),
    resolve_leftovers: { result: scanResult(items) },
  });
  await gotoUninstall(page);

  await expect(page.getByRole("button", { name: "卸载", exact: true })).toBeVisible();
  await expect(page.getByRole("searchbox", { name: "搜索应用" })).toBeVisible();
  await page.getByRole("button", { name: /Safari/ }).click();
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeVisible();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(overflow).toBe(false);
});
