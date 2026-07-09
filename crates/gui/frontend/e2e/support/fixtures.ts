/**
 * E2E 夹具（U2 / R2 / R3）。事件流与命令响应工厂，类型逐字复用 `ipc.ts` 导出（KTD4）——
 * 夹具形状漂移即 TS 报错。含 `defaultHandlers()`：被测流程启动即触发的命令默认值
 * （check_fda authorized 是 U4/U5 到达主界面的前置；path 插件供 Analyze 的 userHome）。
 */
import type {
  ProgressEvent,
  AnalyzeEvent,
  ScanResult,
  ScanItem,
  CategoryGroup,
  CleanReport,
  DirNode,
  FdaStatus,
  SafetyLevel,
  PathSafety,
} from "../../src/lib/ipc";
import type { Handlers, HandlerSpec } from "./tauri-mock";

// ---- 单项 / 分组构造 ----

export function scanItem(
  path: string,
  size: number,
  opts: Partial<Pick<ScanItem, "category" | "safety" | "selected" | "impact" | "recovery">> = {},
): ScanItem {
  const safety: SafetyLevel = opts.safety ?? "Safe";
  return {
    path,
    size,
    safety,
    category: opts.category ?? "系统缓存",
    // 安全模型不变量：Risky 永不预选；其余默认预选（对齐 upsertFound 语义）。
    selected: opts.selected ?? safety !== "Risky",
    impact: opts.impact ?? "占用磁盘空间",
    recovery: opts.recovery ?? "下次使用时自动重建",
  };
}

export function scanResult(items: ScanItem[]): ScanResult {
  const byCat = new Map<string, ScanItem[]>();
  for (const it of items) {
    const arr = byCat.get(it.category) ?? [];
    arr.push(it);
    byCat.set(it.category, arr);
  }
  const categories: CategoryGroup[] = [...byCat.entries()].map(([name, its]) => ({
    name,
    items: its,
    total_size: its.reduce((s, i) => s + i.size, 0),
    file_count: its.length,
  }));
  return {
    categories,
    total_size: items.reduce((s, i) => s + i.size, 0),
    file_count: items.length,
  };
}

/** 从一批 ScanItem 生成一条真实形状的扫描事件流：Scanning → N×Found → CategoryDone → Complete。 */
export function scanStream(items: ScanItem[], opts: { error?: string } = {}): ProgressEvent[] {
  const events: ProgressEvent[] = [{ Scanning: { path: "/" } }];
  for (const it of items) {
    events.push({
      Found: {
        category: it.category,
        path: it.path,
        size: it.size,
        safety: it.safety,
        impact: it.impact,
        recovery: it.recovery,
        preselect: it.safety !== "Risky",
      },
    });
  }
  const byCat = new Map<string, ScanItem[]>();
  for (const it of items) byCat.set(it.category, [...(byCat.get(it.category) ?? []), it]);
  for (const [name, its] of byCat) {
    events.push({ CategoryDone: { category: name, total_size: its.reduce((s, i) => s + i.size, 0), count: its.length } });
  }
  if (opts.error) events.push({ Error: opts.error });
  events.push("Complete");
  return events;
}

/** clean/delete 的流：N×CleaningFile → CleaningDone。 */
export function cleanStream(paths: string[], freed: number, deleted?: string[]): ProgressEvent[] {
  const events: ProgressEvent[] = paths.map((p) => ({ CleaningFile: { path: p } }));
  events.push({ CleaningDone: { freed, count: (deleted ?? paths).length, deleted_paths: deleted ?? paths } });
  return events;
}

export function cleanReport(paths: string[], sizePer: number, opts: { fail?: string[] } = {}): CleanReport {
  const fail = new Set(opts.fail ?? []);
  const cleaned = paths.map((p) => ({
    path: p,
    size: sizePer,
    success: !fail.has(p),
    error: fail.has(p) ? "权限不足" : null,
  }));
  const success = cleaned.filter((c) => c.success);
  return {
    cleaned,
    total_freed: success.reduce((s, c) => s + c.size, 0),
    success_count: success.length,
    failure_count: cleaned.length - success.length,
  };
}

// ---- Analyze 树 ----

export function dirNode(
  path: string,
  name: string,
  size: number,
  opts: { is_file?: boolean; children?: DirNode[] } = {},
): DirNode {
  return { path, name, size, is_file: opts.is_file ?? false, children: opts.children ?? [] };
}

/** analyze 流：Progress → Finished。 */
export function analyzeStream(fileCount: number, totalSize: number): AnalyzeEvent[] {
  return [{ Progress: { file_count: fileCount, total_size: totalSize } }, "Finished"];
}

// ---- FDA ----

export function fdaAuthorized(): FdaStatus {
  return { authorized: true, probes: [{ path: "/Library/Caches", status: { status: "readable" } }] };
}

export function fdaUnauthorized(): FdaStatus {
  return {
    authorized: false,
    probes: [
      { path: "/Library/Caches", status: { status: "no_permission" } },
      { path: "~/Library/Caches", status: { status: "readable" } },
    ],
  };
}

export function pathSafety(paths: string[], risky: string[] = []): PathSafety[] {
  const riskySet = new Set(risky);
  return paths.map((path) => ({ path, safety: (riskySet.has(path) ? "Risky" : "Safe") as SafetyLevel }));
}

const FAKE_HOME = "/Users/tester";

/**
 * 被测流程启动即触发的命令默认值。测试用 `{ ...defaultHandlers(), <cmd>: <spec> }` 覆盖需要的项。
 * - check_fda authorized：App 挂载即调，决定能否进主界面（U4/U5 前置）。
 * - plugin:path|resolve_directory：Analyze 的 userHome() → homeDir() 走此插件命令（feasibility review）。
 * - cancel_scan/open_trash/open_fda_settings：无副作用默认，避免未注册 reject。
 */
export function defaultHandlers(): Handlers {
  return {
    check_fda: { result: fdaAuthorized() },
    scan_clean: { events: scanStream([]), result: scanResult([]) },
    clean: { events: [], result: cleanReport([], 0) },
    analyze: { events: analyzeStream(0, 0), result: dirNode(FAKE_HOME, "tester", 0) },
    classify_marked: { result: [] as PathSafety[] },
    delete_marked: { events: [], result: cleanReport([], 0) },
    cancel_scan: { result: null },
    open_trash: { result: null },
    open_fda_settings: { result: null },
    reveal_in_finder: { result: null },
    "plugin:path|resolve_directory": { result: FAKE_HOME },
  };
}

export { FAKE_HOME };
export type { HandlerSpec };
