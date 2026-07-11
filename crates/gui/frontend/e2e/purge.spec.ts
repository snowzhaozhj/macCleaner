import { test, expect, type Page } from "@playwright/test";
import { installTauriMock, lastCall, callsFor } from "./support/tauri-mock";
import {
  defaultHandlers,
  scanStream,
  scanResult,
  scanItem,
  cleanStream,
  cleanReport,
  FAKE_HOME,
  type HandlerSpec,
} from "./support/fixtures";

// move 7 第一段 Purge E2E（plan 020 / AE1–AE7）。主干：选目录→扫描→聚合→删除信任链；
// 关键分支：选择器取消、Risky type-to-confirm、扫描取消、clean/purge 隔离、720×520 布局。

const MB = 1024 * 1024;

/** 进入「开发清理」tab（三 tab 导航，U5）。 */
async function gotoPurge(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "开发清理" }).click();
}

function devItems() {
  return [
    scanItem(`${FAKE_HOME}/code/web/node_modules`, 500 * MB, { category: "Node.js" }),
    scanItem(`${FAKE_HOME}/code/mc/target`, 300 * MB, { category: "Rust" }),
  ];
}

function purgeScanHandlers(items = devItems()): Record<string, HandlerSpec> {
  return {
    ...defaultHandlers(),
    scan_purge: { events: scanStream(items), result: scanResult(items) },
  };
}

test("AE1 默认 ~ 扫描：idle 显示主目录为目标，开始扫描以 ~ 为根并按 purge 分类聚合", async ({ page }) => {
  await installTauriMock(page, purgeScanHandlers());
  await gotoPurge(page);

  // idle 态：目标目录可见 = 主目录；未自动开扫（与 Clean 的自动扫描不同）。
  await expect(page.getByText(FAKE_HOME, { exact: true })).toBeVisible();
  expect(await lastCall(page, "scan_purge")).toBeNull();

  await page.getByRole("button", { name: "开始扫描" }).click();

  const call = await lastCall(page, "scan_purge");
  expect(call?.args.path).toBe(FAKE_HOME);
  expect(call?.args.onEvent).toBe("[Channel]");

  // 完成 settle：命中的分类行可见，0 命中分类收拢（防跳变契约的完成态）。
  await expect(page.getByRole("button", { name: /Node\.js/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Rust/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Docker/ })).toHaveCount(0);
  // 首屏「可安全释放」= 预选合计 800 MiB（formatBytes ≥100 取整数位）。
  await expect(page.getByText("800 MiB").first()).toBeVisible();
});

test("AE2 选择目录成功：目标路径更新并作为后续扫描根", async ({ page }) => {
  const workspace = `${FAKE_HOME}/code`;
  await installTauriMock(page, {
    ...purgeScanHandlers(),
    "plugin:dialog|open": { result: workspace },
  });
  await gotoPurge(page);

  await page.getByRole("button", { name: "选择目录" }).click();
  await expect(page.getByText(workspace, { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "开始扫描" }).click();
  const call = await lastCall(page, "scan_purge");
  expect(call?.args.path).toBe(workspace);
});

test("AE3 选择器取消：目标保持原值、停留 idle、无错误提示", async ({ page }) => {
  // defaultHandlers 的 dialog 默认 resolve null（用户取消）。
  await installTauriMock(page, purgeScanHandlers());
  await gotoPurge(page);
  await expect(page.getByText(FAKE_HOME, { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "选择目录" }).click();

  await expect(page.getByText(FAKE_HOME, { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
  expect(await lastCall(page, "scan_purge")).toBeNull();
});

test("AE7 纯 Safe/Moderate 删除：无确认模态，purge 空口令触发，回执+撤销吐司", async ({ page }) => {
  const items = devItems();
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...purgeScanHandlers(items),
    purge: { events: cleanStream(paths, 800 * MB), result: cleanReport(paths, 400 * MB) },
  });
  await gotoPurge(page);
  await page.getByRole("button", { name: "开始扫描" }).click();

  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  await expect(page.getByRole("dialog")).toHaveCount(0);
  const call = await lastCall(page, "purge");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe("");
  expect(call?.args.onEvent).toBe("[Channel]");

  await expect(page.getByText(/已释放/)).toBeVisible();
  await expect(page.getByRole("status")).toContainText("已移到废纸篓");
});

test("AE4 Risky 项：默认未预选不入删除批次；显式勾选后须 type-to-confirm，purge 带口令", async ({ page }) => {
  const safe = scanItem(`${FAKE_HOME}/code/web/node_modules`, 500 * MB, { category: "Node.js" });
  const risky = scanItem(`${FAKE_HOME}/code/app/.gradle`, 200 * MB, {
    category: "Gradle",
    safety: "Risky",
    selected: false,
  });
  const items = [safe, risky];
  await installTauriMock(page, {
    ...purgeScanHandlers(items),
    purge: {
      events: cleanStream([safe.path, risky.path], 700 * MB),
      result: cleanReport([safe.path, risky.path], 350 * MB),
    },
  });
  await gotoPurge(page);
  await page.getByRole("button", { name: "开始扫描" }).click();

  // Risky 未预选：删除按钮只计 Safe 项体积（500 MiB，无 Risky 的 200）。
  await expect(page.getByRole("button", { name: /移入废纸篓 · 释放 500 MiB/ })).toBeVisible();

  // 显式勾选 Risky → 删除 → type-to-confirm 模态。
  await page.getByRole("button", { name: /Gradle/ }).click();
  await page.getByRole("checkbox", { name: risky.path }).check();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const deleteBtn = dialog.getByRole("button", { name: "删除" });
  await expect(deleteBtn).toBeDisabled();
  await dialog.getByRole("textbox").fill("delete");
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  const call = await lastCall(page, "purge");
  expect(call?.args.confirmToken).toBe("delete");
  expect(call?.args.paths).toContain(risky.path);
});

test("AE6 扫描中取消：cancel_scan 触发，部分项清空回 idle 无残留", async ({ page }) => {
  // 流入部分 Found 后保持悬挂：取消须把这些部分项清掉（后端已拒绝取消结果写 last_purge，
  // 保留会形成「可见但删除必然落空」的假结果）。
  const partial = devItems();
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_purge: { events: scanStream(partial).slice(0, -1), result: scanResult(partial), pending: true },
  });
  await gotoPurge(page);
  await page.getByRole("button", { name: "开始扫描" }).click();

  const cancelBtn = page.getByRole("button", { name: "取消" });
  await expect(cancelBtn).toBeVisible();
  await cancelBtn.click();

  expect(await lastCall(page, "cancel_scan")).not.toBeNull();
  // 回 idle：可重新开扫，且流式部分项不残留。
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Node\.js/ })).toHaveCount(0);
});

test("扫描 Error 事件：结果态显示错误横幅而非静默", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_purge: { events: scanStream([], { error: "扫描失败：目录不可读" }), result: scanResult([]) },
  });
  await gotoPurge(page);
  await page.getByRole("button", { name: "开始扫描" }).click();

  await expect(page.getByRole("alert")).toContainText("扫描失败：目录不可读");
});

test("R5 选择器失败(reject)：目标保持原值、停留 idle、无错误提示", async ({ page }) => {
  await installTauriMock(page, {
    ...purgeScanHandlers(),
    "plugin:dialog|open": { error: "选择器异常" },
  });
  await gotoPurge(page);
  await expect(page.getByText(FAKE_HOME, { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "选择目录" }).click();

  await expect(page.getByText(FAKE_HOME, { exact: true })).toBeVisible();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
  expect(await lastCall(page, "scan_purge")).toBeNull();
});

test("results 相位改选目录：旧结果作废回 idle，不出现「标签新目录、删旧目录项」", async ({ page }) => {
  const workspace = `${FAKE_HOME}/code`;
  await installTauriMock(page, {
    ...purgeScanHandlers(),
    "plugin:dialog|open": { result: workspace },
  });
  await gotoPurge(page);
  await page.getByRole("button", { name: "开始扫描" }).click();
  await expect(page.getByRole("button", { name: /Node\.js/ })).toBeVisible();

  await page.getByRole("button", { name: "选择目录" }).click();

  // 目标更新为新目录，旧目录的结果行与删除按钮全部清空，回 idle 待重扫。
  await expect(page.getByText(workspace, { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Node\.js/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
});

test("AE5 隔离（前端面）：clean 与 purge 各自删除各自扫描的项", async ({ page }) => {
  const cleanItems = [scanItem("/Library/Caches/sys", 10 * MB, { category: "系统缓存" })];
  const purgeItems = devItems();
  const purgePaths = purgeItems.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(cleanItems), result: scanResult(cleanItems) },
    scan_purge: { events: scanStream(purgeItems), result: scanResult(purgeItems) },
    purge: { events: cleanStream(purgePaths, 800 * MB), result: cleanReport(purgePaths, 400 * MB) },
  });
  // 先进 Clean（自动扫描），再切开发清理扫描并删除。
  await page.goto("/");
  await expect(page.getByRole("button", { name: /系统缓存/ })).toBeVisible();
  await page.getByRole("button", { name: "开发清理" }).click();
  await page.getByRole("button", { name: "开始扫描" }).click();
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 删除走 purge 命令且只带 purge 扫描的路径；clean 命令未被触发。
  const call = await lastCall(page, "purge");
  expect(call?.args.paths).toEqual(purgePaths);
  expect(await callsFor(page, "clean")).toEqual([]);
});

test("三 tab 导航：切换互不串扰，purge 进入 idle 不自动扫描", async ({ page }) => {
  await installTauriMock(page, purgeScanHandlers());
  await page.goto("/");

  await expect(page.getByRole("button", { name: "清理", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "开发清理" })).toBeVisible();
  await expect(page.getByRole("button", { name: "分析" })).toBeVisible();

  await page.getByRole("button", { name: "开发清理" }).click();
  await expect(page.getByText("开发清理模式")).toBeVisible();
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
  expect(await lastCall(page, "scan_purge")).toBeNull();

  // 切回清理再切回来：purge 重回 idle（独立生命周期，R2）。
  await page.getByRole("button", { name: "清理", exact: true }).click();
  await page.getByRole("button", { name: "开发清理" }).click();
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();
});

test("720×520 最小窗口：三 tab 与 purge 操作可见可点、无横向滚动", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 520 });
  await installTauriMock(page, purgeScanHandlers());
  await gotoPurge(page);

  await expect(page.getByRole("button", { name: "开发清理" })).toBeVisible();
  await expect(page.getByRole("button", { name: "选择目录" })).toBeVisible();
  await expect(page.getByRole("button", { name: "开始扫描" })).toBeVisible();

  await page.getByRole("button", { name: "开始扫描" }).click();
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeVisible();

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(overflow).toBe(false);
});
