import { test, expect } from "@playwright/test";
import {
  callsFor,
  installTauriMock,
  lastCall,
  releaseDeferred,
} from "./support/tauri-mock";
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
  await page.getByRole("button", { name: "清理", exact: true }).click();
  await expect(page.getByText("可安全释放")).toBeVisible();
});

test("审查多开、三手势独立，折叠重开复用单路径证据", async ({ page }) => {
  const movies = "/Users/tester/Movies";
  const archive = "/Users/tester/big.zip";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      sequence: [
        { result: pathSafety([movies]) },
        { result: pathSafety([archive], [archive]) },
      ],
    },
  });

  const moviesReview = page.getByRole("button", { name: `审查 ${movies}` });
  const archiveReview = page.getByRole("button", { name: `审查 ${archive}` });
  await moviesReview.click();
  await archiveReview.click();
  await expect(moviesReview).toHaveAttribute("aria-expanded", "true");
  await expect(archiveReview).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByText(movies, { exact: true })).toBeVisible();
  await expect(page.getByText(archive, { exact: true })).toBeVisible();
  expect((await callsFor(page, "classify_marked")).map((c) => c.args.paths)).toEqual([
    [movies],
    [archive],
  ]);

  // 审查不标记、不进入；checkbox 与进入仍是独立手势。
  await expect(page.getByRole("checkbox", { name: movies })).not.toBeChecked();
  await page.getByRole("checkbox", { name: movies }).check();
  await expect(moviesReview).toHaveAttribute("aria-expanded", "true");
  await moviesReview.click();
  await moviesReview.click();
  await expect(page.getByText("应用缓存被清空，下次使用时自动重建").first()).toBeVisible();
  expect(await callsFor(page, "classify_marked")).toHaveLength(2);
});

test("加载中折叠后保留结果缓存，重开不重复请求", async ({ page }) => {
  const target = "/Users/tester/big.zip";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      deferred: "folded-review",
      result: pathSafety([target], [target]),
    },
  });

  const review = page.getByRole("button", { name: `审查 ${target}` });
  await review.click();
  await expect(page.getByRole("status", { name: `正在查询 ${target} 的删除安全证据` })).toBeVisible();
  await review.click();
  await releaseDeferred(page, "folded-review");
  await review.click();
  await expect(page.getByText("不可恢复（除非保留了对应构建的 dSYM 备份）")).toBeVisible();
  expect(await callsFor(page, "classify_marked")).toHaveLength(1);
});

test("导航卸载旧同路径实例，旧返回不污染新实例", async ({ page }) => {
  const repeated = "/Users/tester/Movies/repeated";
  const tree = dirNode(FAKE_HOME, "tester", 800 * MB, {
    children: [
      dirNode("/Users/tester/Movies", "Movies", 400 * MB, {
        children: [dirNode(repeated, "repeated", 200 * MB, { is_file: true })],
      }),
      dirNode(repeated, "repeated", 200 * MB, { is_file: true }),
    ],
  });
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: tree },
    classify_marked: {
      sequence: [
        {
          deferred: "old-instance",
          result: pathSafety([repeated], [repeated], {
            [repeated]: { impact: "旧实例证据", recovery: "旧实例恢复" },
          }),
        },
        {
          deferred: "new-instance",
          result: pathSafety([repeated], [], {
            [repeated]: { impact: "新实例证据", recovery: "新实例恢复" },
          }),
        },
      ],
    },
  });

  await page.getByRole("button", { name: `审查 ${repeated}` }).click();
  await page.getByRole("button", { name: "进入 Movies" }).click();
  await page.getByRole("button", { name: `审查 ${repeated}` }).click();
  await releaseDeferred(page, "new-instance");
  await expect(page.getByText("新实例证据")).toBeVisible();
  await releaseDeferred(page, "old-instance");
  await expect(page.getByText("新实例证据")).toBeVisible();
  await expect(page.getByText("旧实例证据")).toHaveCount(0);
});

test("审查失败 fail-closed，可重试且乱序只接受最后请求", async ({ page }) => {
  const target = "/Users/tester/Documents";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      sequence: [
        { error: "分类服务暂不可用" },
        {
          deferred: "retry-old",
          result: pathSafety([target], [target], {
            [target]: { impact: "较旧重试证据", recovery: "较旧恢复" },
          }),
        },
        {
          deferred: "retry-new",
          result: pathSafety([target], [], {
            [target]: { impact: "最后重试证据", recovery: "最后恢复" },
          }),
        },
      ],
    },
  });

  await page.getByRole("button", { name: `审查 ${target}` }).click();
  const alert = page.getByRole("alert").filter({ hasText: target });
  await expect(alert).toContainText("分类服务暂不可用");
  await expect(page.getByText("分类服务不可用", { exact: false })).toBeVisible();
  await expect(page.getByText("未匹配内置清理规则", { exact: true })).toHaveCount(0);
  const retry = page.getByRole("button", { name: `重新查询 ${target}` });
  await retry.click();
  await expect(retry).toBeDisabled();
  // 用户态禁用可阻止重复提交；合成事件覆盖请求令牌的乱序防御。
  await retry.evaluate((button) => button.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  await releaseDeferred(page, "retry-new");
  await expect(page.getByText("最后重试证据")).toBeVisible();
  await releaseDeferred(page, "retry-old");
  await expect(page.getByText("最后重试证据")).toBeVisible();
  await expect(page.getByText("较旧重试证据")).toHaveCount(0);
});

test("空集、漏回与成功未知路径保持各自的 fail-closed 证据边界", async ({ page }) => {
  const empty = "/Users/tester/Documents";
  const missing = "/Users/tester/big.zip";
  const unknown = "/Users/tester/Movies";
  const impact = "此路径未匹配任何已知清理规则，删除可能造成用户数据丢失";
  const recovery = "请先核对内容；若仍在废纸篓可移回原处";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      sequence: [
        { result: [] },
        { result: pathSafety(["/Users/tester/other"]) },
        {
          result: pathSafety([unknown], [unknown], {
            [unknown]: { impact, recovery },
          }),
        },
      ],
    },
  });

  for (const target of [empty, missing]) {
    await page.getByRole("button", { name: `审查 ${target}` }).click();
    const panel = page.getByRole("region", { name: `${target} 的删除审查` });
    await expect(panel.getByTitle("安全等级：危险")).toBeVisible();
    await expect(panel.getByRole("alert")).toContainText("分类结果未包含目标路径");
    await expect(panel.getByRole("button", { name: `重新查询 ${target}` })).toBeVisible();
    await expect(panel.getByText("未匹配内置清理规则", { exact: true })).toHaveCount(0);
  }

  await page.getByRole("button", { name: `审查 ${unknown}` }).click();
  const unknownPanel = page.getByRole("region", { name: `${unknown} 的删除审查` });
  await expect(unknownPanel.getByTitle("安全等级：危险")).toBeVisible();
  await expect(unknownPanel.getByText(impact, { exact: true })).toBeVisible();
  await expect(unknownPanel.getByText(recovery, { exact: true })).toBeVisible();
  await expect(unknownPanel.getByText("未匹配内置清理规则", { exact: true })).toBeVisible();
  await expect(unknownPanel.getByRole("alert")).toHaveCount(0);
});

test("CLI 随当前目录审查层显示；Finder 失败保留审查和标记", async ({ page }) => {
  const target = "/Users/tester/Movies";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target]) },
    reveal_in_finder: { error: "路径不存在" },
  });

  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(0);
  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: `审查 ${target}` }).click();
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(1);
  await expect(page.getByText("mc analyze '/Users/tester'", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: `在 Finder 中显示 ${target}` }).click();
  await expect(page.getByRole("alert").filter({ hasText: target })).toContainText("路径不存在");
  await expect(page.getByRole("checkbox", { name: target })).toBeChecked();
  await expect(page.getByRole("button", { name: `审查 ${target}` })).toHaveAttribute("aria-expanded", "true");
  expect((await lastCall(page, "reveal_in_finder"))?.args.path).toBe(target);

  await page.getByRole("button", { name: "进入 Movies" }).click();
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: target, exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "tester", exact: true }).click();
  await expect(page.getByRole("checkbox", { name: target, exact: true })).toBeChecked();
  await page.getByRole("button", { name: `审查 ${target}` }).click();
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(1);
  await page.getByRole("button", { name: "重新分析" }).click();
  await expect(page.getByRole("checkbox", { name: target, exact: true })).not.toBeChecked();
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(0);
});

test("Finder 乱序响应只保留最后一次动作结果", async ({ page }) => {
  const target = "/Users/tester/Movies";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target]) },
    reveal_in_finder: {
      sequence: [
        { deferred: "finder-old", error: "较旧 Finder 失败" },
        { deferred: "finder-new", result: null },
      ],
    },
  });

  await page.getByRole("button", { name: `审查 ${target}` }).click();
  const finder = page.getByRole("button", { name: `在 Finder 中显示 ${target}` });
  await finder.click();
  await expect(finder).toBeDisabled();
  await finder.evaluate((button) => button.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  await releaseDeferred(page, "finder-new");
  await expect(finder).toBeEnabled();
  await releaseDeferred(page, "finder-old");
  await expect(page.getByRole("alert").filter({ hasText: target })).toHaveCount(0);
  expect(await callsFor(page, "reveal_in_finder")).toHaveLength(2);
});

test("删除已展开节点后清理审查面孔与 CLI 提示", async ({ page }) => {
  const target = "/Users/tester/Library/Caches";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      sequence: [
        { result: pathSafety([target]) },
        { result: pathSafety([target]) },
      ],
    },
    delete_marked: {
      events: cleanStream([target], 100 * MB),
      result: cleanReport([target], 100 * MB),
    },
  });

  await page.getByRole("button", { name: `审查 ${target}` }).click();
  await page.getByRole("checkbox", { name: target }).check();
  await expect(page.getByRole("region", { name: `${target} 的删除审查` })).toBeVisible();
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(1);
  await page.getByRole("button", { name: "删除标记" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "删除" }).click();

  await expect(page.getByRole("checkbox", { name: target })).toHaveCount(0);
  await expect(page.getByRole("region", { name: `${target} 的删除审查` })).toHaveCount(0);
  await expect(page.getByText("在命令行继续分析此目录")).toHaveCount(0);
});

test("审查证据不授权删除，确认前再次分类并采用升级后的 Risky", async ({ page }) => {
  const target = "/Users/tester/Library/Caches";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: {
      sequence: [
        { result: pathSafety([target]) },
        { result: pathSafety([target], [target]) },
      ],
    },
  });

  await page.getByRole("button", { name: `审查 ${target}` }).click();
  await expect(page.getByTitle("安全等级：安全")).toBeVisible();
  await page.getByRole("checkbox", { name: target }).check();
  await page.getByRole("button", { name: "删除标记" }).click();
  await expect(page.getByRole("dialog").getByRole("button", { name: "删除" })).toBeDisabled();
  expect((await callsFor(page, "classify_marked")).map((c) => c.args.paths)).toEqual([
    [target],
    [target],
  ]);
});

test("720×520 审查态无横向滚动且控件可见", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 520 });
  const target = "/Users/tester/Movies";
  await gotoAnalyzeReady(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(10, 800 * MB), result: sampleTree() },
    classify_marked: { result: pathSafety([target]) },
  });
  await page.getByRole("button", { name: `审查 ${target}` }).click();
  const row = page.getByRole("listitem").filter({ has: page.getByRole("checkbox", { name: target }) });
  const enter = row.getByRole("button", { name: "进入 Movies" });
  const size = row.getByText("300 MiB", { exact: true });
  await expect(enter).toBeVisible();
  await expect(enter).toBeEnabled();
  await expect(size).toBeVisible();
  await expect(row.getByRole("button", { name: `审查 ${target}` })).toBeVisible();
  await expect(page.getByRole("button", { name: `在 Finder 中显示 ${target}` })).toBeVisible();
  for (const control of [enter, size]) {
    const box = await control.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.x + box!.width).toBeLessThanOrEqual(720);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
