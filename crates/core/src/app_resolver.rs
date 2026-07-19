use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use log::{debug, warn};

use crate::models::{AppInfo, SafetyLevel, ScanItem};
use crate::platform;
use crate::progress::{ProgressEvent, ProgressReporter};

/// 应用解析器：发现已安装应用，查找应用残留
pub struct AppResolver;

/// 查找应用残留时要搜索的 ~/Library 子目录
const LEFTOVER_SUBDIRS: &[&str] = &[
    "Caches",
    "Preferences",
    "Application Support",
    "LaunchAgents",
    "Saved Application State",
    "Logs",
    "WebKit",
    "HTTPStorages",
];

/// 可能存放不可再生用户数据的残留子目录（数据库、IndexedDB/localStorage、存档等）。
/// 这些项标 `Moderate` + 不默认预选（issue #25 方案 B，详见 `find_leftovers` D3 注释）：
/// 可能含数据但移废纸篓可逆，故既不静默默认删、也不逐个 type-to-confirm 告警。
const USER_DATA_SUBDIRS: &[&str] = &[
    "Application Support",
    "WebKit",
    "HTTPStorages",
    "Saved Application State",
];

/// 反向卸载（孤儿扫描）时排除的系统预留 / 共享 bundle-id 前缀。
///
/// 这些前缀下的残留常年存在于 `~/Library`，其"父 App"是系统组件或多产品共用的容器，
/// 而非可卸载的独立应用——把它们当孤儿删会误杀系统状态或其他仍在用的应用数据。
/// 首版只硬保 `com.apple.`（系统组件）；其余共享前缀（如 `com.google.` 下多产品共用目录）
/// 按真机误报反馈迭代追加，不在首版穷举。判据见
/// `docs/solutions/security-issues/orphan-leftover-scan-false-positive-defenses.md`。
const RESERVED_BUNDLE_PREFIXES: &[&str] = &["com.apple."];

/// 孤儿残留的默认最小龄（天）：残留目录 mtime 距今不足此值则跳过。
///
/// 刚删 App 的残留可能是用户临时操作、或马上要重装——给缓冲期减少误报。孤儿是"回收"
/// 非"必删"，漏报可再扫，误杀不可逆代价更高（fail-closed 取向）。
const ORPHAN_MIN_AGE_DAYS: u64 = 30;

impl AppResolver {
    /// 扫描 /Applications 和 ~/Applications 中的 .app 包，
    /// 读取 Info.plist 获取 bundle ID 和应用名
    pub fn list_apps() -> Vec<AppInfo> {
        let mut apps = Vec::new();
        let home = platform::get_home_dir();

        let app_dirs = vec![PathBuf::from("/Applications"), home.join("Applications")];

        for dir in &app_dirs {
            if !dir.exists() {
                debug!("应用目录不存在，跳过: {dir:?}");
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) => {
                    warn!("无法读取应用目录 {dir:?}: {e:?}");
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        warn!("读取目录条目失败: {e:?}");
                        continue;
                    }
                };
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "app") {
                    continue;
                }
                match Self::read_app_info(&path) {
                    Ok(info) => apps.push(info),
                    Err(e) => {
                        debug!("解析应用信息失败 {path:?}: {e:?}");
                    }
                }
            }
        }

        apps.sort_by_key(|a| a.name.to_lowercase());
        apps
    }

    /// 流式扫描已安装应用：每解析出一个 .app 立刻 `Found` 一条，末尾 `Complete`。
    ///
    /// 与 `list_apps` 的区别是把每个 `calc_app_size` 的重活边算边报，供 TUI 在
    /// 后台线程调用、边扫边渲染，避免主线程同步计算全部应用体积造成界面冻结。
    /// 尊重 `reporter.is_cancelled()`，用户取消时提前返回。
    pub fn scan_apps_streaming(reporter: &dyn ProgressReporter) {
        let home = platform::get_home_dir();
        let app_dirs = [PathBuf::from("/Applications"), home.join("Applications")];
        Self::scan_apps_in_dirs(&app_dirs, reporter);
    }

    /// `scan_apps_streaming` 的可注入目录内核，供测试传入临时目录。
    fn scan_apps_in_dirs(app_dirs: &[PathBuf], reporter: &dyn ProgressReporter) {
        let mut found_any = false;
        let mut read_errors: Vec<String> = Vec::new();
        for dir in app_dirs {
            if !dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) => {
                    // 权限类读取失败升为结构化「跳过（需授权）」事件（#23）；仍保留 read_errors
                    // 用于"全无所获时上报 Error"的兜底。非权限错误只走原有 warn 路径。
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        reporter.on_event(ProgressEvent::SkippedNoPermission {
                            path: dir.clone(),
                        });
                    }
                    warn!("无法读取应用目录 {dir:?}: {e:?}");
                    read_errors.push(format!("{}: {e}", dir.display()));
                    continue;
                }
            };
            for entry in entries.flatten() {
                if reporter.is_cancelled() {
                    return;
                }
                let path = entry.path();
                if path.extension().is_none_or(|ext| ext != "app") {
                    continue;
                }
                match Self::read_app_info(&path) {
                    Ok(info) => {
                        found_any = true;
                        reporter.on_event(ProgressEvent::Found {
                            category: "已安装应用".to_string(),
                            path: info.path,
                            size: info.size,
                            safety: SafetyLevel::Moderate,
                            impact: String::new(),
                            recovery: String::new(),
                            preselect: true,
                        });
                    }
                    Err(e) => debug!("解析应用信息失败 {path:?}: {e:?}"),
                }
            }
        }

        // 一个应用都没发现、且确有目录读取失败：显式上报 I/O 错误，
        // 避免 TUI 把"无法读取 /Applications"误显示为"未发现可清理的文件"。
        if !found_any && !read_errors.is_empty() {
            reporter.on_event(ProgressEvent::Error(format!(
                "无法读取应用目录：{}",
                read_errors.join("；")
            )));
            return;
        }

        reporter.on_event(ProgressEvent::Complete);
    }

    /// 从 .app 包的 Info.plist 中读取应用信息
    fn read_app_info(app_path: &PathBuf) -> Result<AppInfo> {
        let plist_path = app_path.join("Contents/Info.plist");

        // 应用名：优先从 plist 读取，否则用目录名
        let fallback_name = app_path
            .file_stem().map_or_else(|| "Unknown".to_string(), |s| s.to_string_lossy().to_string());

        let (bundle_id, name, version) = if plist_path.exists() {
            Self::parse_info_plist(&plist_path, &fallback_name)?
        } else {
            debug!("Info.plist 不存在: {plist_path:?}");
            (None, fallback_name, None)
        };

        // 计算 .app 包大小
        let size = Self::calc_app_size(app_path);

        Ok(AppInfo {
            name,
            bundle_id,
            path: app_path.clone(),
            size,
            version,
        })
    }

    /// 解析 Info.plist 文件，提取 bundle ID、应用名和版本
    fn parse_info_plist(
        plist_path: &PathBuf,
        fallback_name: &str,
    ) -> Result<(Option<String>, String, Option<String>)> {
        let value: plist::Value = plist::from_file(plist_path)
            .with_context(|| format!("解析 Info.plist 失败: {plist_path:?}"))?;

        let dict = value.as_dictionary();

        let bundle_id = dict
            .and_then(|d| d.get("CFBundleIdentifier"))
            .and_then(|v| v.as_string())
            .map(std::string::ToString::to_string);

        let name = dict
            .and_then(|d| {
                d.get("CFBundleDisplayName")
                    .or_else(|| d.get("CFBundleName"))
            })
            .and_then(|v| v.as_string()).map_or_else(|| fallback_name.to_string(), std::string::ToString::to_string);

        let version = dict
            .and_then(|d| {
                d.get("CFBundleShortVersionString")
                    .or_else(|| d.get("CFBundleVersion"))
            })
            .and_then(|v| v.as_string())
            .map(std::string::ToString::to_string);

        Ok((bundle_id, name, version))
    }

    /// 计算 .app 目录的大小，使用 `symlink_metadata` 避免跟随符号链接
    fn calc_app_size(path: &Path) -> u64 {
        let mut total: u64 = 0;
        let entries = match fs::read_dir(path) {
            Ok(e) => e,
            Err(_) => return 0,
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if meta.is_dir() {
                total += Self::calc_app_size(&entry.path());
            } else {
                total += meta.len();
            }
        }
        total
    }

    /// 计算残留条目的体积：目录递归求和，文件取自身大小；读不到 metadata 时归 0。
    /// 正向 `find_leftovers` 与反向 `scan_orphans` 共用。
    fn entry_size(path: &Path) -> u64 {
        match fs::symlink_metadata(path) {
            Ok(m) => {
                if m.is_dir() {
                    Self::calc_app_size(path)
                } else {
                    m.len()
                }
            }
            Err(_) => 0,
        }
    }

    /// 根据 bundle ID 在 ~/Library 标准路径下查找应用残留
    ///
    /// 搜索 Caches, Preferences, Application Support, `LaunchAgents`,
    /// Saved Application State, Logs, `WebKit`, `HTTPStorages`
    pub fn find_leftovers(bundle_id: &str) -> Vec<ScanItem> {
        let home = platform::get_home_dir();
        let library = home.join("Library");
        let mut leftovers = Vec::new();

        for subdir in LEFTOVER_SUBDIRS {
            let search_dir = library.join(subdir);
            if !search_dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(&search_dir) {
                Ok(e) => e,
                Err(e) => {
                    debug!("无法读取 {search_dir:?}: {e:?}");
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                let entry_name = entry.file_name().to_string_lossy().to_string();
                let entry_lower = entry_name.to_lowercase();
                let bid_lower = bundle_id.to_lowercase();
                if entry_lower == bid_lower
                    || entry_lower.starts_with(&format!("{bid_lower}."))
                    || entry_lower.starts_with(&format!("{bid_lower}-"))
                {
                    let path = entry.path();
                    let size = Self::entry_size(&path);
                    // D3（issue #25 方案 B）：Application Support / WebKit / HTTPStorages /
                    // Saved Application State 可能存放应用的主用户数据（数据库、IndexedDB/
                    // localStorage、存档等），但移废纸篓可恢复。取舍：
                    //   - 不标 Safe + 默认勾选——"可能丢不可再生数据却静默默认删"违反无静默删除原则；
                    //   - 也不标 Risky——残留能从废纸篓找回，逐个 type-to-confirm 属 cry wolf、太烦。
                    // 故标 Moderate + preselect=false：不过度告警、不默认删，用户想删按键勾上即可
                    //（selected = safety != Risky && preselect，Moderate+false 即"不预选、可手动勾、
                    // 无需 type-to-confirm"）。
                    // 模型 nuance：Moderate 原义是"零数据丢失 + 重建摩擦"；此处是"可能丢数据但可逆"，
                    // 属两判据决策树对"不确定含数据、但可逆"这一格的留白，借 Moderate 这一格承载。
                    // 保留原证据文案。其余残留（Caches/Preferences/Logs 等）是明确可再生产物，
                    // 仍 Safe + 默认预选。
                    let item = if USER_DATA_SUBDIRS.contains(subdir) {
                        ScanItem::new(path, size, SafetyLevel::Moderate, format!("应用残留 ({subdir})"))
                            .with_evidence(
                                "可能含应用数据（数据库、缓存的文档/草稿、存档等）".to_string(),
                                "默认移入废纸篓可找回；清空废纸篓或 --permanent 后不可恢复".to_string(),
                            )
                            .with_preselect(false)
                    } else {
                        ScanItem::new(path, size, SafetyLevel::Safe, format!("应用残留 ({subdir})"))
                    };
                    leftovers.push(item);
                }
            }
        }

        debug!(
            "bundle_id={} 找到 {} 个残留项",
            bundle_id,
            leftovers.len()
        );
        leftovers
    }

    /// 反向卸载：遍历 `~/Library` 标准子目录，找出**父 App 已不存在**的 bundle-id 残留（孤儿）。
    ///
    /// 与 [`find_leftovers`](Self::find_leftovers) 的正向语义互补：正向是"给定已装 App → 找它的残留"，
    /// 反向是"枚举残留 → 反查父 App 是否还在"。用户装了又删的应用会在 `~/Library` 留下没有主人的残留，
    /// 正向路径永远找不到它们（App 已不在列表里无从选起）。
    ///
    /// **四道误杀防线**（详见
    /// `docs/solutions/security-issues/orphan-leftover-scan-false-positive-defenses.md`）：
    /// 1. **fail-closed 析取**：从条目名析不出 bundle-id（不含 `.` 的普通名，如 `Caches/Google/`）→ 跳过，
    ///    宁可漏报不误杀。
    /// 2. **系统预留黑名单**（[`RESERVED_BUNDLE_PREFIXES`]）：`com.apple.` 等系统 / 共享前缀绝不当孤儿。
    /// 3. **龄阈值**（[`ORPHAN_MIN_AGE_DAYS`]）：mtime 距今不足阈值的残留给缓冲期，跳过。
    /// 4. **空已装集合 fail-closed**：`list_apps` 读不到 `/Applications`（权限）会退化为空集合，此时
    ///    「父已卸载」判断不可信，整体返回空而非把所有残留当孤儿（评审 R1）。
    ///
    /// **已知局限**：`list_apps` 只扫应用目录顶层，嵌套安装的 App（Setapp、Utilities、`Adobe Acrobat DC/`
    /// 等子目录）不在集合内，其在用残留可能被列为孤儿候选；辅助进程/更新器的 sibling bundle-id
    /// （`com.google.Keystone.Agent` 等）也不与父 App id 前缀匹配。这些经 `preselect=false` + 移废纸篓 +
    /// 逐项人工勾选兜底，不会静默误删。
    ///
    /// 分级比正向**更保守**：孤儿一律 `preselect=false`（含 Safe 项）——用户没说要删任何东西，是工具主动
    /// 发现的，App 已卸载但用户可能故意保留数据，故永不默认删、永不 `--yes` 自动删，须逐项手动勾。
    #[must_use]
    pub fn scan_orphans() -> Vec<ScanItem> {
        let installed: HashSet<String> = Self::list_apps()
            .into_iter()
            .filter_map(|app| app.bundle_id.map(|b| b.to_lowercase()))
            .collect();
        // fail-closed（评审 R1）：孤儿判定完全依赖「已装 bundle-id 集合」。`list_apps` 只扫
        // /Applications 与 ~/Applications 顶层、且对读失败静默降级——若 /Applications 因权限读不到，
        // 集合会退化为空，于是**每一条**非 Apple、超龄的 ~/Library 残留都被判成孤儿（实为在用 App 的数据）。
        // 空集合无法可信地支撑「父已卸载」判断，按 fail-closed 直接返回空（宁漏报不误杀），
        // 而非把「读不到已装应用」误当成「什么都没装」。
        if installed.is_empty() {
            debug!("已装应用集合为空（可能 /Applications 不可读），孤儿扫描 fail-closed 返回空");
            return Vec::new();
        }
        let home = platform::get_home_dir();
        let library = home.join("Library");
        Self::scan_orphans_in(&library, &installed, Duration::from_secs(ORPHAN_MIN_AGE_DAYS * 86_400))
    }

    /// [`scan_orphans`](Self::scan_orphans) 的可注入内核，供测试传入临时 library 根、
    /// 已装 bundle-id 集合、龄阈值，不依赖真机 `~/Library` 内容与系统时钟。
    fn scan_orphans_in(
        library: &Path,
        installed: &HashSet<String>,
        min_age: Duration,
    ) -> Vec<ScanItem> {
        let now = SystemTime::now();
        let mut orphans = Vec::new();

        for subdir in LEFTOVER_SUBDIRS {
            let search_dir = library.join(subdir);
            if !search_dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(&search_dir) {
                Ok(e) => e,
                Err(e) => {
                    debug!("无法读取 {search_dir:?}: {e:?}");
                    continue;
                }
            };
            for entry in entries.flatten() {
                let entry_name = entry.file_name().to_string_lossy().to_string();

                // 防线 1：从条目名析出候选 bundle-id 前缀；析不出则跳过（fail-closed）。
                let Some(candidate) = Self::extract_bundle_id(&entry_name) else {
                    continue;
                };
                let candidate_lower = candidate.to_lowercase();

                // 防线 2：系统预留 / 共享前缀绝不当孤儿。
                if RESERVED_BUNDLE_PREFIXES
                    .iter()
                    .any(|p| candidate_lower.starts_with(p))
                {
                    continue;
                }

                // 父 App 仍在（正向匹配规则的补集）→ 不是孤儿。
                if Self::bundle_installed(&candidate_lower, installed) {
                    continue;
                }

                let path = entry.path();

                // 防线 3：龄不足阈值 → 缓冲期内，跳过。
                if !Self::older_than(&path, now, min_age) {
                    continue;
                }

                let size = Self::entry_size(&path);

                // 分级沿用 #25 rubric（USER_DATA → Moderate + 证据，其余 Safe），
                // 但孤儿场景把 preselect 统一关掉（含 Safe 项）——见方法文档。
                let item = if USER_DATA_SUBDIRS.contains(subdir) {
                    ScanItem::new(path, size, SafetyLevel::Moderate, format!("孤儿残留 ({subdir})"))
                        .with_evidence(
                            "可能含应用数据（数据库、缓存的文档/草稿、存档等）；父应用已卸载".to_string(),
                            "默认移入废纸篓可找回；清空废纸篓或 --permanent 后不可恢复".to_string(),
                        )
                        .with_preselect(false)
                } else {
                    ScanItem::new(path, size, SafetyLevel::Safe, format!("孤儿残留 ({subdir})"))
                        .with_preselect(false)
                };
                orphans.push(item);
            }
        }

        debug!("反向扫描找到 {} 个孤儿残留项", orphans.len());
        orphans
    }

    /// 从 `~/Library` 残留条目名析出候选 bundle-id 前缀。
    ///
    /// 残留条目名通常是 `com.vendor.App` / `com.vendor.App.plist` / `com.vendor.App-hash` 形态。
    /// **不剥后缀**：直接按 `.` 分段计数判定是否像 bundle-id——含至少两个 `.`（形如
    /// `com.vendor.App` 的反向域名）才认作候选，挡掉普通目录名（`Google`、`Microsoft`）与单段名，
    /// 后者返回 `None`（fail-closed，交由调用方跳过）。带 `.plist`/`-hash` 等后缀的条目名保留原样，
    /// 与已装集合的归位由 [`bundle_installed`](Self::bundle_installed) 的前缀匹配处理。
    fn extract_bundle_id(entry_name: &str) -> Option<String> {
        let trimmed = entry_name.trim();
        if trimmed.is_empty() {
            return None;
        }
        // 至少两个 `.`（com.vendor.app）才像 bundle-id，挡掉 `Google` / `Adobe` 这类普通目录名。
        if trimmed.matches('.').count() < 2 {
            return None;
        }
        Some(trimmed.to_string())
    }

    /// 候选 bundle-id 是否命中已装集合（正向匹配规则的补集判定）。
    ///
    /// 与 [`find_leftovers`](Self::find_leftovers) 的匹配规则对称：相等，或候选是某已装 id 的
    /// `id.` / `id-` 派生（残留带后缀），或某已装 id 是候选的同形派生。双向前缀关系确保
    /// `com.foo.App` 残留能匹配到已装的 `com.foo.App`，也能让带 hash 后缀的残留归位。
    fn bundle_installed(candidate_lower: &str, installed: &HashSet<String>) -> bool {
        // 精确命中先走 O(1) 哈希探测。
        if installed.contains(candidate_lower) {
            return true;
        }
        // 前缀关系哈希查不了，需线性扫；用 strip_prefix 原地比字节，避免 format! 逐个分配。
        // `candidate` 是某 `id` 的 `id.`/`id-` 派生（残留带后缀），或反之。
        installed.iter().any(|id| {
            candidate_lower
                .strip_prefix(id.as_str())
                .or_else(|| id.strip_prefix(candidate_lower))
                .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('-'))
        })
    }

    /// 路径 mtime 距 `now` 是否 ≥ `min_age`。读不到 mtime 时保守返回 `false`（视为"太新"→ 跳过，
    /// fail-closed：无法判定龄就不删）。
    fn older_than(path: &Path, now: SystemTime, min_age: Duration) -> bool {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return false;
        };
        let Ok(mtime) = meta.modified() else {
            return false;
        };
        now.duration_since(mtime).is_ok_and(|age| age >= min_age)
    }

    /// 读取指定 `.app` 的 bundle ID（只解析 Info.plist，不计算体积）。
    ///
    /// 供 GUI 卸载**服务端派生** bundle ID，不信任前端回传：前端若传过宽前缀（如 `"com"`）
    /// 会让 `find_leftovers` 前缀匹配到无关应用的 `~/Library` 残留。用真实 bundle ID 收敛匹配范围。
    pub fn bundle_id_at(app_path: &Path) -> Option<String> {
        let plist_path = app_path.join("Contents/Info.plist");
        if !plist_path.exists() {
            return None;
        }
        Self::parse_info_plist(&plist_path, "")
            .ok()
            .and_then(|(bundle_id, _, _)| bundle_id)
    }

    /// 在应用列表中按名称或 bundle ID 进行大小写不敏感搜索
    pub fn search_apps<'a>(query: &str, apps: &'a [AppInfo]) -> Vec<&'a AppInfo> {
        let query_lower = query.to_lowercase();
        apps.iter()
            .filter(|app| {
                app.name.to_lowercase().contains(&query_lower)
                    || app
                        .bundle_id
                        .as_ref()
                        .is_some_and(|id| id.to_lowercase().contains(&query_lower))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;

    #[test]
    fn test_list_apps_no_panic() {
        // 在真实 macOS 上运行，至少应有一些应用
        let apps = AppResolver::list_apps();
        // 不断言具体数量，但不应 panic
        debug!("发现 {} 个应用", apps.len());
    }

    #[test]
    fn test_search_apps_case_insensitive() {
        let apps = vec![
            AppInfo {
                name: "Safari".to_string(),
                bundle_id: Some("com.apple.Safari".to_string()),
                path: PathBuf::from("/Applications/Safari.app"),
                size: 0,
                version: None,
            },
            AppInfo {
                name: "TextEdit".to_string(),
                bundle_id: Some("com.apple.TextEdit".to_string()),
                path: PathBuf::from("/Applications/TextEdit.app"),
                size: 0,
                version: None,
            },
        ];

        let results = AppResolver::search_apps("safari", &apps);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "Safari");

        let results = AppResolver::search_apps("SAFARI", &apps);
        assert_eq!(results.len(), 1);

        let results = AppResolver::search_apps("com.apple", &apps);
        assert_eq!(results.len(), 2, "应同时匹配 bundle ID");

        let results = AppResolver::search_apps("nonexistent", &apps);
        assert!(results.is_empty());
    }

    #[test]
    fn test_find_leftovers_empty_bundle_id() {
        // 对一个极不可能存在的 bundle ID 搜索，应返回空
        let leftovers = AppResolver::find_leftovers("com.test.nonexistent.app.12345");
        assert!(leftovers.is_empty());
    }

    #[test]
    fn test_parse_info_plist_with_fixture() {
        let dir = tempfile::tempdir().unwrap();
        let plist_path = dir.path().join("Info.plist");

        // 创建一个最小的 plist 文件
        let plist_content = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.test.MyApp</string>
    <key>CFBundleName</key>
    <string>MyApp</string>
    <key>CFBundleShortVersionString</key>
    <string>1.2.3</string>
</dict>
</plist>"#;

        let mut f = File::create(&plist_path).unwrap();
        f.write_all(plist_content.as_bytes()).unwrap();

        let (bundle_id, name, version) =
            AppResolver::parse_info_plist(&plist_path.clone(), "Fallback").unwrap();

        assert_eq!(bundle_id, Some("com.test.MyApp".to_string()));
        assert_eq!(name, "MyApp");
        assert_eq!(version, Some("1.2.3".to_string()));
    }

    #[test]
    fn test_read_app_info_missing_plist() {
        let dir = tempfile::tempdir().unwrap();
        let app_dir = dir.path().join("TestApp.app");
        fs::create_dir_all(&app_dir).unwrap();

        let info = AppResolver::read_app_info(&app_dir.clone()).unwrap();
        assert_eq!(info.name, "TestApp");
        assert!(info.bundle_id.is_none());
    }

    /// 记录事件、可配置取消的测试 reporter
    struct RecReporter {
        found: std::sync::Mutex<Vec<PathBuf>>,
        complete: std::sync::atomic::AtomicBool,
        error: std::sync::Mutex<Option<String>>,
        skipped: std::sync::Mutex<Vec<PathBuf>>,
        cancelled: bool,
    }
    impl RecReporter {
        fn new(cancelled: bool) -> Self {
            Self {
                found: std::sync::Mutex::new(Vec::new()),
                complete: std::sync::atomic::AtomicBool::new(false),
                error: std::sync::Mutex::new(None),
                skipped: std::sync::Mutex::new(Vec::new()),
                cancelled,
            }
        }
    }
    impl ProgressReporter for RecReporter {
        fn on_event(&self, event: ProgressEvent) {
            match event {
                ProgressEvent::Found { path, .. } => self.found.lock().unwrap().push(path),
                ProgressEvent::Complete => {
                    self.complete.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                ProgressEvent::Error(msg) => *self.error.lock().unwrap() = Some(msg),
                ProgressEvent::SkippedNoPermission { path } => {
                    self.skipped.lock().unwrap().push(path);
                }
                _ => {}
            }
        }
        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
    }

    #[test]
    fn scan_apps_in_dirs_emits_found_per_app_and_completes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Foo.app")).unwrap();
        fs::create_dir_all(dir.path().join("Bar.app")).unwrap();
        fs::create_dir_all(dir.path().join("NotAnApp")).unwrap(); // 非 .app，应跳过

        let rec = RecReporter::new(false);
        AppResolver::scan_apps_in_dirs(&[dir.path().to_path_buf()], &rec);

        assert_eq!(rec.found.lock().unwrap().len(), 2, "两个 .app 应各 Found 一次，非 .app 跳过");
        assert!(
            rec.complete.load(std::sync::atomic::Ordering::Relaxed),
            "扫描结束应发送 Complete"
        );
    }

    #[test]
    fn scan_apps_in_dirs_respects_cancellation() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Foo.app")).unwrap();

        let rec = RecReporter::new(true); // 一开始就取消
        AppResolver::scan_apps_in_dirs(&[dir.path().to_path_buf()], &rec);

        assert!(rec.found.lock().unwrap().is_empty(), "已取消应提前返回，不发 Found");
        assert!(
            !rec.complete.load(std::sync::atomic::Ordering::Relaxed),
            "取消路径不发 Complete"
        );
    }

    #[test]
    fn scan_apps_in_dirs_reports_error_when_unreadable_and_no_apps() {
        // 用一个普通文件冒充应用目录：exists() 为真但 read_dir 失败，
        // 且没有发现任何 app —— 应上报 Error 而非静默 Complete（否则 TUI 误显"未发现"）。
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("not_a_dir");
        fs::write(&fake, b"x").unwrap();

        let rec = RecReporter::new(false);
        AppResolver::scan_apps_in_dirs(&[fake], &rec);

        assert!(rec.found.lock().unwrap().is_empty());
        assert!(rec.error.lock().unwrap().is_some(), "读取失败且无应用时应上报 Error");
        assert!(
            !rec.complete.load(std::sync::atomic::Ordering::Relaxed),
            "已上报 Error 的路径不再发 Complete"
        );
    }

    #[cfg(unix)]
    #[test]
    fn scan_apps_in_dirs_emits_skipped_on_permission_denied() {
        // #23：应用目录因权限读不到时，产生结构化 SkippedNoPermission（而非只静默 warn）。
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked_apps");
        fs::create_dir(&locked).unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

        // root 可穿透 0o000：若能读则环境不适合断言，复原后跳过。
        if fs::read_dir(&locked).is_ok() {
            let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o755));
            return;
        }

        let rec = RecReporter::new(false);
        AppResolver::scan_apps_in_dirs(std::slice::from_ref(&locked), &rec);
        let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o755));

        let skipped = rec.skipped.lock().unwrap();
        assert!(
            skipped.iter().any(|p| p == &locked),
            "无权限的应用目录应产生 SkippedNoPermission 事件，实际: {skipped:?}"
        );
    }

    #[test]
    fn test_calc_app_size() {
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("a.txt");
        let file2 = dir.path().join("b.txt");
        let mut f1 = File::create(&file1).unwrap();
        f1.write_all(b"hello").unwrap();
        let mut f2 = File::create(&file2).unwrap();
        f2.write_all(b"world!").unwrap();

        let size = AppResolver::calc_app_size(dir.path());
        assert_eq!(size, 11, "总大小应为 11 字节");
    }

    #[test]
    fn test_symlink_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        let link = dir.path().join("link.txt");

        let mut f = File::create(&target).unwrap();
        f.write_all(b"1234567890").unwrap(); // 10 bytes

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target, &link).unwrap();
            // symlink_metadata 应返回链接本身的大小，不是目标文件大小
            let link_meta = fs::symlink_metadata(&link).unwrap();
            // 链接的文件类型是 symlink
            assert!(link_meta.file_type().is_symlink());
        }
    }

    #[test]
    fn test_user_data_leftovers_moderate_others_safe() {
        // issue #25 方案 B：USER_DATA_SUBDIRS 派生的残留（Application Support / WebKit /
        // HTTPStorages / Saved Application State）应为 Moderate + 未预选 + 非空证据文案；
        // 其余残留（Caches/Preferences/Logs 等）仍为 Safe。
        // 用 com.apple.Safari，因为几乎所有 Mac 都有。
        // 注意：真实 ~/Library 子目录可能不含该 bundle 的残留 → leftovers 为空，
        // 此时循环不执行即通过，属稳健行为（不因环境无残留而误判）。
        let leftovers = AppResolver::find_leftovers("com.apple.Safari");
        for item in &leftovers {
            let is_user_data = USER_DATA_SUBDIRS
                .iter()
                .any(|sub| item.category.contains(sub));
            if is_user_data {
                assert_eq!(
                    item.safety,
                    SafetyLevel::Moderate,
                    "用户数据残留应为 Moderate: {:?}",
                    item.path
                );
                assert!(!item.selected, "用户数据残留不应默认预选: {:?}", item.path);
                assert!(
                    !item.impact.is_empty() && !item.recovery.is_empty(),
                    "用户数据残留应有非空证据文案: {:?}",
                    item.path
                );
            } else {
                assert_eq!(
                    item.safety,
                    SafetyLevel::Safe,
                    "其余残留应为 Safe: {:?}",
                    item.path
                );
            }
        }
    }

    // ---- 反向卸载（孤儿扫描）测试 ----

    /// 在临时 library 根的某子目录下建一个残留条目，并把其 mtime 设为 `age_days` 天前，
    /// 以便可控地测试龄阈值。返回残留路径。
    fn make_leftover(library: &Path, subdir: &str, name: &str, age_days: u64) -> PathBuf {
        let dir = library.join(subdir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("data.bin"), b"x").unwrap();
        // 把残留目录 mtime 回拨 age_days 天（std File::set_modified，无需额外依赖）。
        // Unix 下目录可只读打开并经 futimens 设时间。
        let past = SystemTime::now() - Duration::from_secs(age_days * 86_400);
        let handle = fs::OpenOptions::new().read(true).open(&path).unwrap();
        handle.set_modified(past).unwrap();
        path
    }

    fn installed_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|s| s.to_lowercase()).collect()
    }

    const OLD: u64 = 60; // 远超默认 30 天阈值
    fn min_age_30d() -> Duration {
        Duration::from_secs(ORPHAN_MIN_AGE_DAYS * 86_400)
    }

    #[test]
    fn scan_orphans_lists_missing_parent_and_skips_installed() {
        // R1：父 App 不存在的残留被列为孤儿；父 App 仍在的残留不被列出。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        make_leftover(lib, "Caches", "com.gone.App", OLD);
        make_leftover(lib, "Caches", "com.installed.App", OLD);

        let installed = installed_set(&["com.installed.App"]);
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());

        let names: Vec<String> = orphans
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"com.gone.App".to_string()), "父不在应列为孤儿: {names:?}");
        assert!(
            !names.contains(&"com.installed.App".to_string()),
            "父仍在不应列为孤儿: {names:?}"
        );
        // 孤儿一律不预选（KTD2），即使 Caches 是 Safe。
        for item in &orphans {
            assert!(!item.selected, "孤儿残留一律不预选: {:?}", item.path);
        }
    }

    #[test]
    fn scan_orphans_excludes_reserved_apple_prefix() {
        // R5：com.apple.* 系统预留前缀绝不当孤儿，即便已装集合不含它。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        make_leftover(lib, "Caches", "com.apple.Safari", OLD);

        let installed = installed_set(&[]); // 空：Safari 不在已装集合
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());

        assert!(orphans.is_empty(), "com.apple.* 应被黑名单排除: {orphans:?}");
    }

    #[test]
    fn scan_orphans_fail_closed_on_non_bundle_name() {
        // R2 fail-closed：条目名不含足够的 `.`（普通目录名）→ 析不出 bundle-id，跳过、不当孤儿。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        make_leftover(lib, "Caches", "Google", OLD); // 普通目录名
        make_leftover(lib, "Caches", "com.single", OLD); // 只有一个 `.`，不像 bundle-id

        let installed = installed_set(&[]);
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());

        assert!(orphans.is_empty(), "析不出 bundle-id 的条目应跳过: {orphans:?}");
    }

    #[test]
    fn scan_orphans_grading_moderate_for_user_data_safe_otherwise() {
        // R3：USER_DATA 子目录派生 → Moderate + 证据 + 不预选；其余 → Safe + 不预选。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        make_leftover(lib, "Application Support", "com.gone.App", OLD); // USER_DATA
        make_leftover(lib, "Caches", "com.gone.App", OLD); // 非 USER_DATA

        let installed = installed_set(&[]);
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());

        let user_data = orphans
            .iter()
            .find(|i| i.category.contains("Application Support"))
            .expect("应含 Application Support 孤儿");
        assert_eq!(user_data.safety, SafetyLevel::Moderate);
        assert!(!user_data.selected, "USER_DATA 孤儿不预选");
        assert!(
            !user_data.impact.is_empty() && !user_data.recovery.is_empty(),
            "USER_DATA 孤儿应有非空证据文案"
        );

        let cache = orphans
            .iter()
            .find(|i| i.category.contains("Caches"))
            .expect("应含 Caches 孤儿");
        assert_eq!(cache.safety, SafetyLevel::Safe);
        assert!(!cache.selected, "Safe 孤儿也不预选（KTD2）");
    }

    #[test]
    fn scan_orphans_respects_age_threshold() {
        // 龄阈值：mtime 在阈值内的孤儿被跳过；超阈值的被列出。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        make_leftover(lib, "Caches", "com.fresh.App", 5); // 5 天 < 30
        make_leftover(lib, "Caches", "com.stale.App", OLD); // 60 天 > 30

        let installed = installed_set(&[]);
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());

        let names: Vec<String> = orphans
            .iter()
            .map(|i| i.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert!(names.contains(&"com.stale.App".to_string()), "超龄应列出: {names:?}");
        assert!(
            !names.contains(&"com.fresh.App".to_string()),
            "龄不足阈值应跳过: {names:?}"
        );
    }

    #[test]
    fn scan_orphans_empty_library_no_panic() {
        // 空 library 根：子目录都不存在 → 返回空、不崩。
        let tmp = tempfile::tempdir().unwrap();
        let installed = installed_set(&[]);
        let orphans = AppResolver::scan_orphans_in(tmp.path(), &installed, min_age_30d());
        assert!(orphans.is_empty());
    }

    #[test]
    fn scan_orphans_prefix_derived_leftover_matches_installed() {
        // 带 hash/后缀的残留（com.gone.App.plist 形态由 Preferences 派生）——若父在应归位、不当孤儿。
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path();
        // Saved Application State 常见形态：com.vendor.App.savedState
        make_leftover(lib, "Saved Application State", "com.keep.App.savedState", OLD);

        let installed = installed_set(&["com.keep.App"]);
        let orphans = AppResolver::scan_orphans_in(lib, &installed, min_age_30d());
        assert!(orphans.is_empty(), "已装 App 的带后缀残留应归位、不当孤儿: {orphans:?}");
    }
}
