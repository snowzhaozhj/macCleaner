/**
 * 后端命令契约集中封装。类型逐字对齐 mc-core serde 外部标签形状。
 * 每个命令一个薄 async 函数；流式命令创建 Channel 并挂 onmessage 回调。
 */
import { invoke, Channel } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";

// ---- 数据形状（serde 外部标签，逐字如下）----

export type SafetyLevel = "Safe" | "Moderate" | "Risky";

export type ProgressEvent =
  | { Scanning: { path: string } }
  | {
      Found: {
        category: string;
        path: string;
        size: number;
        safety: SafetyLevel;
        impact: string;
        recovery: string;
        preselect: boolean;
      };
    }
  | { CategoryDone: { category: string; total_size: number; count: number } }
  | { RuleProgress: { current: number; total: number; name: string } }
  | { SkippedNoPermission: { path: string } }
  | "Complete"
  | { Error: string }
  | { CleaningFile: { path: string } }
  | { CleaningDone: { freed: number; count: number; deleted_paths: string[] } };

export type AnalyzeEvent =
  | { Entry: { name: string; path: string; size: number; is_file: boolean } }
  | { Progress: { file_count: number; total_size: number } }
  | "Finished";

export type DirNode = {
  path: string;
  name: string;
  size: number;
  children: DirNode[];
  is_file: boolean;
};

export type ScanItem = {
  path: string;
  size: number;
  safety: SafetyLevel;
  category: string;
  selected: boolean;
  impact: string;
  recovery: string;
};

export type CategoryGroup = {
  name: string;
  items: ScanItem[];
  total_size: number;
  file_count: number;
};

export type ScanResult = {
  categories: CategoryGroup[];
  total_size: number;
  file_count: number;
};

export type CleanReport = {
  cleaned: { path: string; size: number; success: boolean; error: string | null }[];
  total_freed: number;
  success_count: number;
  failure_count: number;
};

/**
 * clean/purge 命令的响应：清理报告 + 本次账本条目 run_id（供回执一键撤销精确命中）。
 * `run_id` 为 null 表示无可撤销目标（无成功项或写账本失败）——前端据此不显示「撤销清理」，
 * 退回「在访达中恢复」手动路径。
 */
export type CleanResponse = {
  report: CleanReport;
  run_id: string | null;
};

/**
 * 单项恢复状态。逐字对齐 mc-core `RestoreStatus`（serde snake_case）。
 * skipped_* 表示安全跳过（原文件未受影响），与 failed 语义不同、视觉应分列。
 */
export type RestoreStatus =
  | "restored"
  | "skipped_target_occupied"
  | "skipped_trash_missing"
  | "failed";

export type RestoreOutcome = {
  original: string;
  trashed_to: string;
  status: RestoreStatus;
  error: string | null;
};

/**
 * 一次撤销的汇总报告。逐字对齐 mc-core `RestoreReport`——**只序列化 outcomes + dry_run**；
 * `restored_count`/`skipped_count`/`failed_count` 在 Rust 侧是方法而非字段，不过 IPC，
 * 故前端须自行按 status 从 outcomes 派生计数（见 `countRestore`）。
 */
export type RestoreReport = {
  outcomes: RestoreOutcome[];
  dry_run: boolean;
};

/** 从 RestoreReport.outcomes 派生放回/跳过/失败计数（Rust 的计数方法不过 IPC）。 */
export function countRestore(report: RestoreReport): {
  restored: number;
  skipped: number;
  failed: number;
} {
  let restored = 0;
  let skipped = 0;
  let failed = 0;
  for (const o of report.outcomes) {
    if (o.status === "restored") restored += 1;
    else if (o.status === "failed") failed += 1;
    else skipped += 1; // skipped_target_occupied | skipped_trash_missing
  }
  return { restored, skipped, failed };
}

/**
 * 已安装应用信息。字段用 snake_case——mc-core 的 AppInfo 无 #[serde(rename_all)]，
 * Tauri 返回即 snake_case（与本文件其它返回体一致）。写成 bundleId 会运行时 undefined、
 * 导致每个应用都落入「未能解析残留」分支，核心功能静默失效。
 */
export type AppInfo = {
  name: string;
  bundle_id: string | null;
  path: string;
  size: number;
  version: string | null;
};

export type PathStatus =
  | { status: "readable" }
  | { status: "no_permission" }
  | { status: "missing" }
  | { status: "error"; detail: string };

export type ProbeResult = { path: string; status: PathStatus };

export type FdaStatus = { authorized: boolean; probes: ProbeResult[] };

// ---- 命令封装 ----

/** 流式扫描系统/浏览器缓存。onEvent 收到 Found/CategoryDone/… 事件，resolve 时返回最终 ScanResult。 */
export function scanClean(onEvent: (e: ProgressEvent) => void): Promise<ScanResult> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<ScanResult>("scan_clean", { onEvent: channel });
}

/**
 * 删除选中/确认路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传用户输入的确认口令，后端会二次校验（防绕过 type-to-confirm）。
 * 纯非 Risky 删除传空串即可。
 */
export function clean(
  paths: string[],
  confirmToken: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanResponse> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanResponse>("clean", { paths, confirmToken, onEvent: channel });
}

/**
 * 流式扫描 `path` 下的开发产物（node_modules/target/DerivedData…）。
 * 事件与 scanClean 同形（Found/Complete/…），resolve 时返回最终 ScanResult；
 * 结果存入后端独立的 last_purge 槽，与 clean 扫描互不串扰（KTD2）。
 */
export function scanPurge(
  path: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<ScanResult> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<ScanResult>("scan_purge", { path, onEvent: channel });
}

/**
 * 删除 purge 扫描出的选中路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传用户输入的确认口令，后端二次校验（防绕过 type-to-confirm）。
 * 纯非 Risky 删除传空串即可。
 */
export function purge(
  paths: string[],
  confirmToken: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanResponse> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanResponse>("purge", { paths, confirmToken, onEvent: channel });
}

/**
 * 阶段一：列出已安装应用（同步返回，含 bundle_id）。
 * 无流式事件——后端对每个 .app 递归算体积，前端调用期呈加载态。
 */
export function scanUninstall(): Promise<AppInfo[]> {
  return invoke<AppInfo[]>("scan_uninstall");
}

/**
 * 阶段二：对选定应用解析残留，与 app bundle 合成一份 ScanResult 存入后端 last_uninstall 槽。
 * 入参键用 camelCase（Tauri 映射到 Rust snake_case 形参）。bundleId 为 null 时只含 app bundle。
 */
export function resolveLeftovers(
  appPath: string,
  bundleId: string | null,
  appSize: number,
): Promise<ScanResult> {
  return invoke<ScanResult>("resolve_leftovers", { appPath, bundleId, appSize });
}

/**
 * 阶段三：删除卸载审查出的选中路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传确认口令，后端二次校验（防绕过 type-to-confirm）。
 */
export function uninstall(
  paths: string[],
  confirmToken: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanReport> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanReport>("uninstall", { paths, confirmToken, onEvent: channel });
}

/**
 * 反向卸载：同步扫描 `~/Library` 找父 App 已不存在的孤儿残留，存入后端 last_orphans 槽。
 * 无流式事件——核心一次性返回全量快照，前端调用期呈加载态（同 scanUninstall）。
 * 孤儿一律不预选（核心 scan_orphans 保证），前端 selected 映射即天然全未勾。
 */
export function scanOrphans(): Promise<ScanResult> {
  return invoke<ScanResult>("scan_orphans");
}

/**
 * 删除孤儿扫描出的选中路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传确认口令，后端二次校验（孤儿实际不产 Risky，纵深防御）。
 * 纯非 Risky 删除传空串即可。
 */
export function cleanOrphans(
  paths: string[],
  confirmToken: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanResponse> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanResponse>("clean_orphans", { paths, confirmToken, onEvent: channel });
}

/** 协作式取消（scan_clean / scan_purge / clean / analyze / uninstall 通用）。 */
export function cancelScan(): Promise<void> {
  return invoke("cancel_scan");
}

/** 流式分析目录树。onEvent 收到 Entry/Progress/Finished，resolve 时返回 finalize 后的完整树。 */
export function analyze(
  root: string,
  onEvent: (e: AnalyzeEvent) => void,
): Promise<DirNode> {
  const channel = new Channel<AnalyzeEvent>();
  channel.onmessage = onEvent;
  return invoke<DirNode>("analyze", { root, onEvent: channel });
}

/** 一条标记路径的删除分级与证据（未知路径由后端保守归为 Risky）。 */
export type PathSafety = {
  path: string;
  safety: SafetyLevel;
  impact: string;
  recovery: string;
};

/**
 * 为标记路径集回查删除分级与证据（不删除）。前端据此展示真实后果，
 * 并对含 Risky（包括未知路径）的删除要求 type-to-confirm。
 */
export function classifyMarked(paths: string[]): Promise<PathSafety[]> {
  return invoke<PathSafety[]>("classify_marked", { paths });
}

/**
 * 删除 analyze 中标记的路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传用户输入的确认口令，后端二次校验（防绕过 type-to-confirm）。
 * `confirmedRiskyPaths`：确认框中实际展示为 Risky 的路径；后端拒绝确认后才升级的危险项。
 */
export function deleteMarked(
  paths: string[],
  confirmToken: string,
  confirmedRiskyPaths: string[],
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanReport> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanReport>("delete_marked", {
    paths,
    confirmToken,
    confirmedRiskyPaths,
    onEvent: channel,
  });
}

/** 检查完全磁盘访问权限（FDA）。 */
export function checkFda(): Promise<FdaStatus> {
  return invoke<FdaStatus>("check_fda");
}

/** 跳转系统设置 FDA 面板。 */
export function openFdaSettings(): Promise<void> {
  return invoke("open_fda_settings");
}

/** 在 Finder 中打开系统废纸篓（U5「在访达中恢复」——用 Finder 原生「放回原处」恢复）。 */
export function openTrash(): Promise<void> {
  return invoke("open_trash");
}

/**
 * 撤销 `runId` 那次清理：按回执自身 run_id 精确命中账本条目，从废纸篓确定性放回原处。
 * 恒实际执行（非预览）。命中无落点/run_id 不存在 → 返回空报告（outcomes 为空），
 * 调用方据此退回「在访达中恢复」手动降级。
 */
export function undo(runId: string): Promise<RestoreReport> {
  return invoke<RestoreReport>("undo", { runId });
}

/**
 * 在 Finder 中定位并选中某路径（move 6 审查面孔）。只读揭示，不删除/移动。
 * 后端 `open -R`；路径不存在会 reject（前端据此优雅提示，不静默）。
 */
export function revealInFinder(path: string): Promise<void> {
  return invoke("reveal_in_finder", { path });
}

/** 用户主目录（Analyze MVP 默认根）。 */
export function userHome(): Promise<string> {
  return homeDir();
}
