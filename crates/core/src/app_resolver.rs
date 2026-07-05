use std::fs;
use std::path::PathBuf;

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
        for dir in app_dirs {
            if !dir.exists() {
                continue;
            }
            let entries = match fs::read_dir(dir) {
                Ok(entries) => entries,
                Err(e) => {
                    warn!("无法读取应用目录 {dir:?}: {e:?}");
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
                    Ok(info) => reporter.on_event(ProgressEvent::Found {
                        category: "已安装应用".to_string(),
                        path: info.path,
                        size: info.size,
                        safety: SafetyLevel::Moderate,
                    }),
                    Err(e) => debug!("解析应用信息失败 {path:?}: {e:?}"),
                }
            }
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
                    leftovers.push(ScanItem::new(
                        path,
                        size,
                        SafetyLevel::Safe,
                        format!("应用残留 ({subdir})"),
                    ));
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
        cancelled: bool,
    }
    impl RecReporter {
        fn new(cancelled: bool) -> Self {
            Self {
                found: std::sync::Mutex::new(Vec::new()),
                complete: std::sync::atomic::AtomicBool::new(false),
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
    fn test_all_leftovers_marked_safe() {
        // 对一个存在的应用搜索残留，确认全部标记为 Safe
        // 使用 com.apple.Safari 因为几乎所有 Mac 都有
        let leftovers = AppResolver::find_leftovers("com.apple.Safari");
        for item in &leftovers {
            assert_eq!(
                item.safety,
                SafetyLevel::Safe,
                "所有残留项应标记为 Safe: {:?}",
                item.path
            );
        }
    }
}
