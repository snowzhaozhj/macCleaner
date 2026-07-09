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
): Promise<CleanReport> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanReport>("clean", { paths, confirmToken, onEvent: channel });
}

/** 协作式取消（scan_clean / clean / analyze 通用）。 */
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

/** 一条标记路径的安全分级（后端按规则 evidence_for_path 回查）。 */
export type PathSafety = { path: string; safety: SafetyLevel };

/**
 * 为标记路径集回查安全分级（不删除）。前端据此在确认弹窗显示 Risky 三通道，
 * 并对含 Risky 的删除要求 type-to-confirm（分析器项无规则元数据，须回查，R-review）。
 */
export function classifyMarked(paths: string[]): Promise<PathSafety[]> {
  return invoke<PathSafety[]>("classify_marked", { paths });
}

/**
 * 删除 analyze 中标记的路径（恒移废纸篓）。
 * `confirmToken`：含 Risky 项时须传用户输入的确认口令，后端二次校验（防绕过 type-to-confirm）。
 */
export function deleteMarked(
  paths: string[],
  confirmToken: string,
  onEvent: (e: ProgressEvent) => void,
): Promise<CleanReport> {
  const channel = new Channel<ProgressEvent>();
  channel.onmessage = onEvent;
  return invoke<CleanReport>("delete_marked", { paths, confirmToken, onEvent: channel });
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

/** 用户主目录（Analyze MVP 默认根）。 */
export function userHome(): Promise<string> {
  return homeDir();
}
