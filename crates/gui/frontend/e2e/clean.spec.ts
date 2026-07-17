import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall, callsFor, releaseDeferred } from "./support/tauri-mock";
import {
  defaultHandlers,
  scanStream,
  scanResult,
  scanItem,
  cleanStream,
  cleanResponse,
  restoreReport,
  emptyRestoreReport,
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
    clean: { events: cleanStream(paths, 8 * MB), result: cleanResponse(paths, 4 * MB) },
  });
  await page.goto("/");

  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 全 Safe：不弹 type-to-confirm 模态，直删。
  await expect(page.getByRole("dialog")).toHaveCount(0);

  const call = await lastCall(page, "clean");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe(""); // 非 Risky：空口令
  expect(call?.args.onEvent).toBe("[Channel]");

  // 完成：回执 + 撤销吐司（吐司文案含「释放 … 项」，与回执的「项已移到废纸篓」区分）。
  await expect(page.getByText(/已释放/)).toBeVisible();
  await expect(page.getByText(/已移到废纸篓 · 释放/)).toBeVisible();
});

test("撤销清理：点回执『撤销清理』以本次 run_id 触发 undo，渲染放回结果", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: {
      events: cleanStream(paths, 5 * MB),
      result: cleanResponse(paths, 5 * MB, { runId: "run-abc" }),
    },
    undo: { result: restoreReport({ restored: paths }) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // 回执呈现真「撤销清理」（非「在访达中恢复」）。点回执内的撤销（作用域到摘要区，排除同名吐司按钮）。
  const receipt = page.locator(".slot-summary");
  await receipt.getByRole("button", { name: /撤销清理/ }).click();
  const call = await lastCall(page, "undo");
  expect(call?.args.runId).toBe("run-abc");

  // 结果就地播报：已放回 1 项。
  await expect(page.getByText(/已放回/)).toBeVisible();
});

test("撤销降级：undo 返回空报告（无落点）→ 退回『在访达中恢复』触发 open_trash", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: {
      events: cleanStream(paths, 5 * MB),
      result: cleanResponse(paths, 5 * MB, { runId: "run-empty" }),
    },
    undo: { result: emptyRestoreReport() },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const receipt = page.locator(".slot-summary");
  await receipt.getByRole("button", { name: /撤销清理/ }).click();
  expect(await lastCall(page, "undo")).not.toBeNull();

  // 空报告 → 降级提示 + 保留「在访达中恢复」；点它触发 open_trash。
  await receipt.getByRole("button", { name: "在访达中恢复" }).click();
  expect(await lastCall(page, "open_trash")).not.toBeNull();
});

test("撤销三态渲染：放回/跳过/失败分列（评审 #2）", async ({ page }) => {
  const a = "/Library/Caches/a";
  const b = "/Library/Caches/b";
  const c = "/Library/Caches/c";
  const items = [scanItem(a, 5 * MB), scanItem(b, 3 * MB), scanItem(c, 2 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: {
      events: cleanStream(paths, 10 * MB),
      result: cleanResponse(paths, 3 * MB, { runId: "run-3state" }),
    },
    // 一项放回、一项原址占用跳过、一项跨卷失败——三态齐发。
    undo: { result: restoreReport({ restored: [a], skipped: [b], failed: [c] }) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const receipt = page.locator(".slot-summary");
  await receipt.getByRole("button", { name: /撤销清理/ }).click();

  // 计数三态齐显（前端由 outcomes 派生）。
  await expect(receipt.getByText(/已放回/)).toBeVisible();
  await expect(receipt.getByText(/跳过 1/)).toBeVisible();
  await expect(receipt.getByText(/失败 1/)).toBeVisible();
  // 跳过段用中性文案（原文件未受影响），与失败段分列——skip 不能被当作失败呈现。
  await expect(receipt).toContainText("原位置已被占用（原文件未受影响）");
  await expect(receipt).toContainText("1 项放回失败");
});

test("跨入口撤销单飞：吐司与回执共享同一次 undo，绝不二次 restore（评审 #1）", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: {
      events: cleanStream(paths, 5 * MB),
      result: cleanResponse(paths, 5 * MB, { runId: "run-single" }),
    },
    // deferred：undo 进入后悬挂，由测试精确释放，制造「撤销进行中」窗口。
    undo: { deferred: "u1", result: restoreReport({ restored: paths }) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  // done 相位：回执（.slot-summary）与吐司（.toast）同屏，各有一个「撤销清理」按钮。
  await page.locator(".toast").getByRole("button", { name: /撤销清理/ }).click();
  // 撤销在途（deferred 未释放）；再点回执的撤销——共享 undoAction 应合并，不发第二次 undo。
  await page.locator(".slot-summary").getByRole("button", { name: /撤销清理/ }).click();
  await releaseDeferred(page, "u1");

  await expect(page.locator(".slot-summary").getByText(/已放回/)).toBeVisible();
  // 单飞铁律：无论点了几个入口，undo 至多发一次——杜绝二次 restore 的「已放回 0 项·跳过 N」误导。
  expect((await callsFor(page, "undo")).length).toBe(1);
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
      result: cleanResponse([safe.path, risky.path], 7 * MB),
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
    clean: { events: cleanStream(paths, 5 * MB), result: cleanResponse(paths, 5 * MB) },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click();

  const calls = await callsFor(page, "clean");
  expect(calls.length).toBe(1);
  expect(calls[0].args.onEvent).toBe("[Channel]");
});

test("move 6 展开审查面孔：完整路径 + 命令行等价 mc clean + 在 Finder 中显示触发 reveal_in_finder", async ({
  page,
}) => {
  const target = "/Library/Caches/deep/nested/file.bin";
  const items = [
    scanItem(target, 5 * MB, { category: "系统缓存" }),
    scanItem("/Library/Caches/b", 3 * MB, { category: "系统缓存" }),
  ];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
  });
  await page.goto("/");

  // 命令行等价出口：一次呈现、诚实标注（现存真实命令，不假造 --only）。
  await expect(page.getByText("命令行等价")).toBeVisible();
  await expect(page.getByText("mc clean", { exact: true })).toBeVisible();

  // 折叠→展开：点分类头进入审查面孔。
  await page.getByRole("button", { name: /系统缓存/ }).click();

  // 审查面孔：完整路径可见（不截断）。
  await expect(page.getByText(target, { exact: true })).toBeVisible();

  // 「在 Finder 中显示」触发 reveal_in_finder，携带该项完整路径。
  await page
    .getByRole("button", { name: "在 Finder 中显示" })
    .first()
    .click();
  const call = await lastCall(page, "reveal_in_finder");
  expect(call?.args.path).toBe(target);
});

