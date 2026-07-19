import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, lastCall, callsFor } from "./support/tauri-mock";
import {
  defaultHandlers,
  scanResult,
  scanItem,
  cleanStream,
  cleanResponse,
  restoreReport,
  FAKE_HOME,
  type ScanItem,
  type HandlerSpec,
} from "./support/fixtures";

// 孤儿残留（反向卸载）E2E（plan 028 / R1–R7）。主干：进入即扫→审查（一律未勾选 R2）→
// 勾选删除信任链→回执+run_id 撤销；分支：空态、扫描失败态、隔离（clean 结果不入孤儿删除）。

const MB = 1024 * 1024;

/** 进入「孤儿残留」tab（第五 tab，U4）。 */
async function gotoOrphans(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "孤儿残留" }).click();
}

/** 孤儿候选：核心保证一律 preselect=false，故 selected 显式置 false（含 Safe/Moderate）。 */
function orphanItems(): ScanItem[] {
  return [
    scanItem(`${FAKE_HOME}/Library/Caches/com.gone.app`, 120 * MB, {
      category: "应用残留 (Caches)",
      safety: "Safe",
      selected: false,
    }),
    scanItem(`${FAKE_HOME}/Library/Application Support/com.dead.tool`, 80 * MB, {
      category: "应用残留 (Application Support)",
      safety: "Moderate",
      selected: false,
    }),
  ];
}

function orphanScanHandlers(items = orphanItems()): Record<string, HandlerSpec> {
  return {
    ...defaultHandlers(),
    // 同步查询：无事件流，仅 result（区别于 scan_clean/scan_purge 的流式）。
    scan_orphans: { result: scanResult(items) },
  };
}

test("R1 进入即扫描：切到孤儿 tab 自动调 scan_orphans 并按各项 category 列出候选", async ({ page }) => {
  await installTauriMock(page, orphanScanHandlers());
  await gotoOrphans(page);

  await expect(page.getByText("孤儿残留模式")).toBeVisible();
  const call = await lastCall(page, "scan_orphans");
  expect(call).not.toBeNull();

  await expect(page.getByRole("button", { name: /应用残留 \(Caches\)/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /应用残留 \(Application Support\)/ })).toBeVisible();
});

test("R2 一律不预选：初始零勾选，删除按钮无可删目标（禁用），手动勾选文案可见", async ({ page }) => {
  await installTauriMock(page, orphanScanHandlers());
  await gotoOrphans(page);

  // 手动勾选文案（KTD2）。
  await expect(page.getByText(/手动勾选/)).toBeVisible();

  // 首屏「可安全释放」= 预选合计 0（无任何项预选，R2）。
  await expect(page.getByText("0 B").first()).toBeVisible();
  // 删除按钮无选中项时禁用。
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeDisabled();
});

test("R4/R5 勾选删除：空口令触发 clean_orphans，回执+撤销吐司，撤销以 run_id 调 undo", async ({ page }) => {
  const items = orphanItems();
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...orphanScanHandlers(items),
    clean_orphans: {
      events: cleanStream(paths, 200 * MB),
      result: cleanResponse(paths, 200 * MB, { runId: "orphans-run-1" }),
    },
    undo: { result: restoreReport({ restored: paths }) },
  });
  await gotoOrphans(page);

  // 勾选两项（初始全未勾）→ 删除按钮解禁。各项在各自分类下，先展开分类再勾选。
  await page.getByRole("button", { name: /应用残留 \(Caches\)/ }).click();
  await page.getByRole("button", { name: /应用残留 \(Application Support\)/ }).click();
  for (const it of items) {
    await page.getByRole("checkbox", { name: it.path }).check();
  }
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 纯 Safe/Moderate：无确认模态，空口令直删。
  await expect(page.getByRole("dialog")).toHaveCount(0);
  const call = await lastCall(page, "clean_orphans");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe("");
  expect(call?.args.onEvent).toBe("[Channel]");

  await expect(page.getByText(/已释放/)).toBeVisible();
  await expect(page.getByText(/已移到废纸篓 · 释放/)).toBeVisible();

  // 撤销清理：回执按钮以本次 run_id 调 undo（作用域到摘要区，排除同名吐司按钮）。
  await page.locator(".slot-summary").getByRole("button", { name: /撤销清理/ }).click();
  const undoCall = await lastCall(page, "undo");
  expect(undoCall?.args.runId).toBe("orphans-run-1");
  await expect(page.getByText(/已放回/)).toBeVisible();
});

test("R6 空扫描：无孤儿时显示空态文案，不空白、不报错", async ({ page }) => {
  await installTauriMock(page, orphanScanHandlers([]));
  await gotoOrphans(page);

  await expect(page.getByText(/未发现孤儿残留/)).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "重新扫描" })).toBeVisible();
});

test("R6 扫描失败：命令 reject 时显示错误态而非冻结/空白", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_orphans: { error: "扫描孤儿残留失败：~/Library 不可读" },
  });
  await gotoOrphans(page);

  await expect(page.getByRole("alert")).toContainText("扫描孤儿残留失败");
  await expect(page.getByRole("button", { name: "重新扫描" })).toBeVisible();
});

test("R7 隔离（前端面）：clean 结果不进入孤儿删除；删除只带孤儿扫描的路径", async ({ page }) => {
  const cleanItems = [scanItem("/Library/Caches/sys", 10 * MB, { category: "系统缓存" })];
  const orphans = orphanItems();
  const orphanPaths = orphans.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: {
      events: [],
      result: scanResult(cleanItems),
    },
    scan_orphans: { result: scanResult(orphans) },
    clean_orphans: {
      events: cleanStream(orphanPaths, 200 * MB),
      result: cleanResponse(orphanPaths, 200 * MB),
    },
  });
  // 先进 Clean（自动扫描），再切孤儿扫描并删除。
  await page.goto("/");
  await expect(page.getByRole("button", { name: /系统缓存/ })).toBeVisible();
  await page.getByRole("button", { name: "孤儿残留" }).click();

  await page.getByRole("button", { name: /应用残留 \(Caches\)/ }).click();
  await page.getByRole("button", { name: /应用残留 \(Application Support\)/ }).click();
  for (const it of orphans) {
    await page.getByRole("checkbox", { name: it.path }).check();
  }
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 删除走 clean_orphans 且只带孤儿扫描的路径；clean 命令未被触发。
  const call = await lastCall(page, "clean_orphans");
  expect(call?.args.paths).toEqual(orphanPaths);
  expect(await callsFor(page, "clean")).toEqual([]);
});

test("五 tab 导航：孤儿 tab 可见可切，Cmd+K 面板含孤儿导航命令", async ({ page }) => {
  await installTauriMock(page, orphanScanHandlers());
  await page.goto("/");

  await expect(page.getByRole("button", { name: "孤儿残留" })).toBeVisible();

  // Cmd+K 面板导航到孤儿。
  await page.keyboard.press("Meta+k");
  await page.getByRole("textbox").fill("孤儿");
  await page.getByText("孤儿残留").last().click();
  await expect(page.getByText("孤儿残留模式")).toBeVisible();
});
