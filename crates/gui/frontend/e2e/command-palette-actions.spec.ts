import { test, expect } from "@playwright/test";
import { installTauriMock, lastCall } from "./support/tauri-mock";
import {
  defaultHandlers,
  scanStream,
  scanResult,
  scanItem,
  cleanStream,
  cleanReport,
  appInfo,
  dirNode,
  analyzeStream,
} from "./support/fixtures";

// 命令面板路由内动作命令 E2E（U5 / R1–R6）。
// 契约：路由挂载后向面板注册「此刻可执行」的动作命令，随相位/选择态增删，卸载即消失；
// 删除类命令触发既有 primaryDelete/openConfirm 分流，绝不绕过 ConfirmDelete（安全零回归）。

const MB = 1024 * 1024;
const palette = (page: import("@playwright/test").Page) =>
  page.getByRole("dialog", { name: "命令面板" });

async function openPalette(page: import("@playwright/test").Page) {
  await page.keyboard.press("ControlOrMeta+k");
  await expect(palette(page)).toBeVisible();
}

test("R1：切到某路由 → 该路由命令出现在面板；静态 6 命令仍在", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeVisible(); // 到 results

  await openPalette(page);
  // Clean 路由命令 + 静态导航/全局动作并存。
  await expect(page.getByRole("option", { name: "重新扫描", exact: true })).toBeVisible();
  await expect(page.getByRole("option", { name: "打开废纸篓" })).toBeVisible(); // 静态 act.trash
  await expect(page.getByRole("option", { name: "分析", exact: true })).toBeVisible(); // 静态 nav.analyze
});

test("R1：切走 tab → 旧路由命令消失，新路由命令出现", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");

  // 切到 Purge（idle，未选目录）。
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "开发清理", exact: true }).click();
  await openPalette(page);

  // Purge 的「选择目录」在；Clean 的「重新扫描」不在（已卸载）。
  await expect(page.getByRole("option", { name: "选择目录" })).toBeVisible();
  await expect(page.getByRole("option", { name: "重新扫描", exact: true })).toHaveCount(0);
});

test("R2 Clean：扫描中只见『取消扫描』，无『移入废纸篓』", async ({ page }) => {
  await installTauriMock(page, {
    ...defaultHandlers(),
    // 悬挂不 resolve → 保持 scanning 相位。
    scan_clean: { events: scanStream([]), result: scanResult([]), pending: true },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: "取消" })).toBeVisible(); // scanning

  await openPalette(page);
  await expect(page.getByRole("option", { name: "取消扫描" })).toBeVisible();
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toHaveCount(0);
  await expect(page.getByRole("option", { name: "重新扫描", exact: true })).toHaveCount(0);
});

test("R2/R3 Clean：有选中时『移入废纸篓』命令执行 → 面板关闭 + 纯 Safe 直删（空口令，无模态）", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB), scanItem("/Library/Caches/b", 3 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: { events: cleanStream(paths, 8 * MB), result: cleanReport(paths, 4 * MB) },
  });
  await page.goto("/");
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeVisible();

  await openPalette(page);
  await page.getByRole("option", { name: "移入废纸篓" }).click();

  // 面板关闭；全 Safe → 不弹确认模态，clean 以空口令直触发（分流保持，KTD3）。
  await expect(palette(page)).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "命令面板" })).toHaveCount(0);
  const call = await lastCall(page, "clean");
  expect(call?.args.paths).toEqual(paths);
  expect(call?.args.confirmToken).toBe("");
});

test("R2 Purge：idle（默认目标 ~/）→ 见『选择目录』+『开始扫描』；无目标假设不成立时后者应缺席", async ({ page }) => {
  await installTauriMock(page, defaultHandlers());
  await page.goto("/");
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "开发清理", exact: true }).click();

  await openPalette(page);
  // Purge 挂载即默认目标 ~/（与 CLI mc purge 一致）→ idle 即可扫描。
  await expect(page.getByRole("option", { name: "选择目录" })).toBeVisible();
  await expect(page.getByRole("option", { name: "开始扫描" })).toBeVisible();
  // 取消扫描不应在 idle 出现（仅 scanning）。
  await expect(page.getByRole("option", { name: "取消扫描" })).toHaveCount(0);
});

test("R2 Uninstall：listReady 见『重新扫描应用』、无『移入废纸篓』；reviewReady 有选中则出现删除命令", async ({ page }) => {
  const app = appInfo("Foo", 40 * MB);
  const leftovers = scanResult([
    scanItem(app.path, 40 * MB, { category: "应用" }),
    scanItem("/Users/tester/Library/Caches/com.example.foo", 5 * MB, { category: "缓存" }),
  ]);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_uninstall: { result: [app] },
    resolve_leftovers: { result: leftovers },
  });
  await page.goto("/");
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "卸载", exact: true }).click();

  // 阶段一 listReady。
  await openPalette(page);
  await expect(page.getByRole("option", { name: "重新扫描应用" })).toBeVisible();
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  // 选应用进入 reviewReady（残留 Safe 预选）。
  await page.getByText("Foo").first().click();
  await expect(page.getByRole("button", { name: /移入废纸篓/ })).toBeVisible();

  await openPalette(page);
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toBeVisible();
  await expect(page.getByRole("option", { name: "返回应用列表" })).toBeVisible();
});

test("R6/R3 Analyze：删除命令用『删除标记』词汇；执行走 openConfirm 弹 ConfirmDelete（fail-closed 要求口令），delete_marked 未被调用", async ({ page }) => {
  const tree = dirNode("/Users/tester", "tester", 100 * MB, {
    children: [dirNode("/Users/tester/big.bin", "big.bin", 60 * MB, { is_file: true })],
  });
  await installTauriMock(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(1, 100 * MB), result: tree },
    classify_marked: { result: [] }, // 后端未回该路径 → openConfirm fallback 保守归 Risky（fail-closed）
  });
  await page.goto("/");
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "分析", exact: true }).click();

  // idle → 经面板命令「分析主目录」进入 ready（顺带覆盖 idle 相位命令）。
  await openPalette(page);
  await expect(page.getByRole("option", { name: "分析主目录" })).toBeVisible();
  await page.getByRole("option", { name: "分析主目录" }).click();

  // ready：标记一项 → marked.size>0。
  await page.getByRole("checkbox", { name: "/Users/tester/big.bin" }).check();

  await openPalette(page);
  // 词汇一致（R6）：是「删除标记」不是「移入废纸篓」。
  await expect(page.getByRole("option", { name: "删除标记" })).toBeVisible();
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toHaveCount(0);
  await page.getByRole("option", { name: "删除标记" }).click();

  // 面板关闭 → openConfirm 弹 ConfirmDelete；fallback Risky 强制 type-to-confirm（口令输入框在）。
  await expect(palette(page)).toHaveCount(0);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("textbox")).toBeVisible();
  // 未输入口令 → 绝未触发删除 IPC（安全零回归，KTD3）。
  expect(await lastCall(page, "delete_marked")).toBeNull();
});

test("R2 Analyze：无标记时不出现『删除标记』命令", async ({ page }) => {
  const tree = dirNode("/Users/tester", "tester", 10 * MB, {
    children: [dirNode("/Users/tester/x.bin", "x.bin", 6 * MB, { is_file: true })],
  });
  await installTauriMock(page, {
    ...defaultHandlers(),
    analyze: { events: analyzeStream(1, 10 * MB), result: tree },
  });
  await page.goto("/");
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "分析", exact: true }).click();
  await page.getByRole("button", { name: "分析主目录" }).click();

  await openPalette(page);
  await expect(page.getByRole("option", { name: "重新分析" })).toBeVisible();
  await expect(page.getByRole("option", { name: "删除标记" })).toHaveCount(0);
});

test("R2 相位守卫（Clean cleaning）：清理进行中面板不暴露『移入废纸篓』/『重新扫描』（防并发再入）", async ({ page }) => {
  const items = [scanItem("/Library/Caches/a", 5 * MB)];
  const paths = items.map((i) => i.path);
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    // clean 悬挂不 resolve → 停在 cleaning 相位。
    clean: { events: cleanStream(paths, 5 * MB), result: cleanReport(paths, 5 * MB), pending: true },
  });
  await page.goto("/");
  await page.getByRole("button", { name: /移入废纸篓/ }).click(); // → cleaning

  await openPalette(page);
  // cleaning 相位对应按钮只显示进度、无扫描/删除按钮 → 命令也不得暴露（否则并发 doClean/startScan）。
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toHaveCount(0);
  await expect(page.getByRole("option", { name: "重新扫描", exact: true })).toHaveCount(0);
});

test("R2 相位守卫（Purge scanning）：扫描进行中即便 Safe 项预选，也不暴露『移入废纸篓』", async ({ page }) => {
  const items = [scanItem("/Users/tester/proj/node_modules", 20 * MB, { category: "node_modules" })];
  await installTauriMock(page, {
    ...defaultHandlers(),
    // 悬挂：流式 Found 后停在 scanning；Safe 项被预选使 selectedItems>0。
    scan_purge: { events: scanStream(items), result: scanResult(items), pending: true },
  });
  await page.goto("/");
  await page.getByRole("navigation", { name: "功能切换" }).getByRole("button", { name: "开发清理", exact: true }).click();
  await page.getByRole("button", { name: "开始扫描" }).click(); // idle → scanning（悬挂）

  await openPalette(page);
  await expect(page.getByRole("option", { name: "取消扫描" })).toBeVisible();
  // 关键：扫描期预选不得让删除命令泄漏（对应按钮仅 results 渲染）。
  await expect(page.getByRole("option", { name: "移入废纸篓" })).toHaveCount(0);
});

test("R3 安全契约（Clean 含 Risky）：面板『移入废纸篓』命令 → 弹 ConfirmDelete 要求口令、口令框获焦，未输入前 clean 未被调用", async ({ page }) => {
  const safe = scanItem("/Library/Caches/a", 5 * MB, { category: "系统缓存" });
  const risky = scanItem("/Library/Caches/danger", 9 * MB, { category: "系统缓存", safety: "Risky", selected: false });
  const items = [safe, risky];
  await installTauriMock(page, {
    ...defaultHandlers(),
    scan_clean: { events: scanStream(items), result: scanResult(items) },
    clean: { events: cleanStream([safe.path, risky.path], 14 * MB), result: cleanReport([safe.path, risky.path], 7 * MB) },
  });
  await page.goto("/");

  // 展开分类、勾选 Risky（永不预选，须手动选中才进入删除集）。
  await page.getByRole("button", { name: /系统缓存/ }).click();
  await page.getByRole("checkbox", { name: risky.path }).check();

  await openPalette(page);
  await page.getByRole("option", { name: "移入废纸篓" }).click();

  // 含 Risky → 命令不绕过：弹 ConfirmDelete，要求 type-to-confirm。
  await expect(palette(page)).toHaveCount(0);
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const tokenInput = dialog.getByRole("textbox");
  await expect(tokenInput).toBeVisible();
  // 焦点交接（评审 julik-frontend-races）：口令框显式获焦，不因面板关闭的焦点还原而落到背景 tab。
  await expect(tokenInput).toBeFocused();
  // 未输入口令 → clean IPC 绝未被调用（安全契约 KTD3）。
  expect(await lastCall(page, "clean")).toBeNull();

  // 输入正确口令后方可删除，且携带口令。
  await tokenInput.fill("delete");
  await dialog.getByRole("button", { name: "删除" }).click();
  const call = await lastCall(page, "clean");
  expect(call?.args.confirmToken).toBe("delete");
});
