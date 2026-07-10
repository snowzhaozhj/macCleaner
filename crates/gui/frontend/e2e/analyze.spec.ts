import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall } from "./support/tauri-mock";
import {
  defaultHandlers,
  analyzeStream,
  dirNode,
  cleanStream,
  cleanReport,
  pathSafety,
  FAKE_HOME,
} from "./support/fixtures";

// U5 Analyze 流 E2E（R3 / SC5）。主干：分析→树渲染→标记→分级回查→删除；含 Risky type-to-confirm。

const MB = 1024 * 1024;

function sampleTree() {
  return dirNode(FAKE_HOME, "tester", 800 * MB, {
    children: [
      dirNode("/Users/tester/Movies", "Movies", 300 * MB, {
        children: [dirNode("/Users/tester/Movies/x.mov", "x.mov", 300 * MB, { is_file: true })],
      }),
      dirNode("/Users/tester/big.zip", "big.zip", 200 * MB, { is_file: true }),
      dirNode("/Users/tester/Library/Caches", "Caches", 100 * MB),
      dirNode("/Users/tester/Library/Developer/Xcode/Archives", "Archives", 100 * MB),
      dirNode("/Users/tester/Documents", "Documents", 100 * MB),
    ],
  });
}

async function gotoAnalyzeReady(page: import("@playwright/test").Page, handlers = defaultHandlers()) {
  await installTauriMock(page, handlers);
  await page.goto("/");
  // 切到分析 tab（需先经 check_fda authorized 到主界面）。
  await page.getByRole("button", { name: "分析" }).click();
  await page.getByRole("button", { name: "分析主目录" }).click();
}

test("分析：userHome→analyze 以主目录为根，树按体积降序渲染", async ({ page }) => {
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(1234, 500 * MB), result: sampleTree() },
  });

  // 树渲染：两个子节点可见。
  await expect(page.getByRole("checkbox", { name: "/Users/tester/Movies" })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "/Users/tester/big.zip" })).toBeVisible();

  // analyze 以 userHome() 返回的假主目录为根。
  const call = await lastCall(page, "analyze");
  expect(call?.args.root).toBe(FAKE_HOME);
  expect(call?.args.onEvent).toBe("[Channel]");
});

test("标记已知 Safe 项→分级与证据回查→删除：参数精确且无需口令", async ({ page }) => {
  const target = "/Users/tester/Library/Caches";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target]) }, // Safe
    delete_marked: { events: cleanStream([target], 100 * MB), result: cleanReport([target], 100 * MB) },
  });

  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();

  // 分级回查携标记路径。
  const classify = await lastCall(page, "classify_marked");
  expect(classify?.args.paths).toEqual([target]);

  // Safe：确认模态无 type-to-confirm，删除按钮直接可点。
  const dialog = page.getByRole("dialog");
  const deleteBtn = dialog.getByRole("button", { name: "删除" });
  await expect(deleteBtn).toBeEnabled();
  await expect(dialog).toContainText("应用缓存被清空，下次使用时自动重建");
  await expect(dialog).toContainText("无需操作，应用会按需自动重新生成");
  await deleteBtn.click();

  const del = await lastCall(page, "delete_marked");
  expect(del?.args.paths).toEqual([target]);
  expect(del?.args.confirmToken).toBe("");
  expect(del?.args.confirmedRiskyPaths).toEqual([]);
  expect(del?.args.onEvent).toBe("[Channel]");
});

test("标记已知 Risky 项：规则证据可见→强制 type-to-confirm，删除携带口令", async ({ page }) => {
  const target = "/Users/tester/Library/Developer/Xcode/Archives";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target], [target]) }, // Risky
    delete_marked: { events: cleanStream([target], 100 * MB), result: cleanReport([target], 100 * MB) },
  });

  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();

  const dialog = page.getByRole("dialog");
  const deleteBtn = dialog.getByRole("button", { name: "删除" });
  await expect(deleteBtn).toBeDisabled();
  await expect(dialog).toContainText("dSYM");
  await expect(dialog).toContainText("不可恢复");

  await dialog.getByRole("textbox").fill("delete");
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  const del = await lastCall(page, "delete_marked");
  expect(del?.args.confirmToken).toBe("delete");
  expect(del?.args.confirmedRiskyPaths).toEqual([target]);
  expect(del?.args.paths).toEqual([target]);
});

test("标记未知路径：后端保守分为 Risky，确认框展示后果并要求口令", async ({ page }) => {
  const target = "/Users/tester/Documents";
  const impact = "此路径未匹配任何已知清理规则，删除可能造成不可再生的用户数据或应用状态丢失";
  const recovery = "若仍在废纸篓可移回原处；清空废纸篓后，数据可能无法恢复";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      result: pathSafety([target], [target], { [target]: { impact, recovery } }),
    },
  });

  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "删除" })).toBeDisabled();
  await expect(dialog).toContainText(impact);
  await expect(dialog).toContainText(recovery);
});

test("分级结果漏回路径：前端仍 fail-closed 为 Risky，不能直接删除", async ({ page }) => {
  const target = "/Users/tester/Documents";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: [] },
  });

  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();

  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("button", { name: "删除" })).toBeDisabled();
  await expect(dialog).toContainText("无法确认此路径是否可安全删除");
  await expect(dialog).toContainText("若仍在废纸篓，可移回原处");
});

test("Tab 切换 clean↔analyze 不串状态", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");
  await expect(page.getByText("可安全释放")).toBeVisible();
  await page.getByRole("button", { name: "分析" }).click();
  await expect(page.getByRole("button", { name: "分析主目录" })).toBeVisible();
  await page.getByRole("button", { name: "清理" }).click();
  await expect(page.getByText("可安全释放")).toBeVisible();
});
