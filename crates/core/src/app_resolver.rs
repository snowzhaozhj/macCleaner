use std::fs;
use std::path::{Path, PathBuf};

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
    fn calc_app_size(path: &PathBuf) -> u64 {
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
                    let size = match fs::symlink_metadata(&path) {
                        Ok(m) => {
                            if m.is_dir() {
                                Self::calc_app_size(&path)
                            } else {
                                m.len()
                            }
                        }
                        Err(_) => 0,
                    };
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

        let size = AppResolver::calc_app_size(&dir.path().to_path_buf());
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
}
