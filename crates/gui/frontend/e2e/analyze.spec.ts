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
  return dirNode(FAKE_HOME, "tester", 500 * MB, {
    children: [
      dirNode("/Users/tester/Movies", "Movies", 300 * MB, {
        children: [dirNode("/Users/tester/Movies/x.mov", "x.mov", 300 * MB, { is_file: true })],
      }),
      dirNode("/Users/tester/big.zip", "big.zip", 200 * MB, { is_file: true }),
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

test("标记 Safe 项→分级回查→删除：classify_marked 与 delete_marked 参数精确", async ({ page }) => {
  const target = "/Users/tester/big.zip";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 500 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target]) }, // Safe
    delete_marked: { events: cleanStream([target], 200 * MB), result: cleanReport([target], 200 * MB) },
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
  await deleteBtn.click();

  const del = await lastCall(page, "delete_marked");
  expect(del?.args.paths).toEqual([target]);
  expect(del?.args.confirmToken).toBe("");
  expect(del?.args.onEvent).toBe("[Channel]");
});

test("标记 Risky 项：分级回查判 Risky→强制 type-to-confirm，delete_marked 带口令 delete", async ({ page }) => {
  const target = "/Users/tester/big.zip";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 500 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target], [target]) }, // Risky
    delete_marked: { events: cleanStream([target], 200 * MB), result: cleanReport([target], 200 * MB) },
  });

  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();

  const dialog = page.getByRole("dialog");
  const deleteBtn = dialog.getByRole("button", { name: "删除" });
  await expect(deleteBtn).toBeDisabled();

  await dialog.getByRole("textbox").fill("delete");
  await expect(deleteBtn).toBeEnabled();
  await deleteBtn.click();

  const del = await lastCall(page, "delete_marked");
  expect(del?.args.confirmToken).toBe("delete");
  expect(del?.args.paths).toEqual([target]);
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
