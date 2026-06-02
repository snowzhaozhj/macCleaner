use crate::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
use crate::progress::{ProgressEvent, ProgressReporter};
use crate::rules::{clean_rules, purge_rules, CleanRule, PathPattern};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 需要在遍历时跳过的目录名
const SKIP_DIRS: &[&str] = &[".git", ".Spotlight-V100", ".fseventsd"];

pub struct Scanner;

impl Scanner {
    /// 使用 clean 规则扫描（用于 `mc clean` 命令）
    pub fn scan_clean(reporter: &dyn ProgressReporter) -> anyhow::Result<ScanResult> {
        let rules = clean_rules();
        Self::scan_with_rules(&rules, reporter)
    }

    /// 使用 purge 规则扫描指定目录（用于 `mc purge` 命令）
    pub fn scan_purge(
        base_path: &Path,
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<ScanResult> {
        let rules = purge_rules();
        Self::scan_purge_dir(base_path, &rules, reporter)
    }

    /// 按 Exact 规则扫描：遍历每条规则的 Exact 路径下的所有文件
    fn scan_with_rules(
        rules: &[CleanRule],
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<ScanResult> {
        let mut category_map: HashMap<String, Vec<ScanItem>> = HashMap::new();

        for rule in rules {
            for pattern in &rule.patterns {
                if let PathPattern::Exact(base) = pattern {
                    if !base.exists() {
                        continue;
                    }

                    reporter.on_event(ProgressEvent::Scanning {
                        path: base.clone(),
                    });

                    let walker = jwalk::WalkDir::new(base)
                        .skip_hidden(false)
                        .follow_links(false)
                        .process_read_dir(|_depth, _path, _state, children| {
                            children.retain(|entry| {
                                if let Ok(ref e) = entry {
                                    if e.file_type().is_dir() {
                                        let name = e.file_name().to_string_lossy();
                                        return !SKIP_DIRS.contains(&name.as_ref());
                                    }
                                }
                                true
                            });
                        });

                    for entry in walker {
                        let entry = match entry {
                            Ok(e) => e,
                            Err(_) => continue,
                        };

                        // 只统计文件
                        if entry.file_type().is_dir() {
                            continue;
                        }

                        let path = entry.path();
                        let size = std::fs::symlink_metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);

                        reporter.on_event(ProgressEvent::Found {
                            category: rule.category.clone(),
                            path: path.clone(),
                            size,
                        });

                        let item =
                            ScanItem::new(path, size, rule.safety, rule.category.clone());

                        category_map
                            .entry(rule.category.clone())
                            .or_default()
                            .push(item);
                    }
                }
            }
        }

        let result = build_scan_result(category_map, reporter);
        Ok(result)
    }

    /// 按 DirName 规则扫描：遍历 base_path 寻找匹配的目录名
    fn scan_purge_dir(
        base_path: &Path,
        rules: &[CleanRule],
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<ScanResult> {
        let mut category_map: HashMap<String, Vec<ScanItem>> = HashMap::new();

        if !base_path.exists() {
            reporter.on_event(ProgressEvent::Complete);
            return Ok(ScanResult::default());
        }

        reporter.on_event(ProgressEvent::Scanning {
            path: base_path.to_path_buf(),
        });

        // 收集所有 DirName 规则用于匹配（使用 owned 类型以满足 'static）
        let dirname_rules: Vec<(String, SafetyLevel, String)> = rules
            .iter()
            .flat_map(|rule| {
                rule.patterns.iter().filter_map(move |p| {
                    if let PathPattern::DirName(name) = p {
                        Some((name.clone(), rule.safety, rule.category.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect();

        // 同时也收集 Exact 规则（如 DerivedData）
        for rule in rules {
            for pattern in &rule.patterns {
                if let PathPattern::Exact(exact_path) = pattern {
                    if exact_path.exists() && exact_path.starts_with(base_path) {
                        let size = dir_size(exact_path);

                        reporter.on_event(ProgressEvent::Found {
                            category: rule.category.to_string(),
                            path: exact_path.clone(),
                            size,
                        });

                        let item = ScanItem::new(
                            exact_path.clone(),
                            size,
                            rule.safety,
                            rule.category.clone(),
                        );

                        category_map
                            .entry(rule.category.clone())
                            .or_default()
                            .push(item);
                    }
                }
            }
        }

        // 使用 jwalk 遍历 base_path，寻找匹配的 DirName 目录
        // process_read_dir 中只收集路径（不计算大小，避免嵌套 jwalk 死锁）
        // 匹配的目录不再深入扫描
        let matched_paths: std::sync::Arc<std::sync::Mutex<Vec<(PathBuf, SafetyLevel, String)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));

        let dirname_rules_arc = std::sync::Arc::new(dirname_rules);
        let matched_clone = matched_paths.clone();
        let dirname_clone = dirname_rules_arc.clone();

        let walker = jwalk::WalkDir::new(base_path)
            .skip_hidden(false)
            .follow_links(false)
            .process_read_dir(move |_depth, _path, _state, children| {
                children.retain(|entry| {
                    if let Ok(ref e) = entry {
                        if e.file_type().is_dir() {
                            let name = e.file_name().to_string_lossy();

                            // 跳过系统特殊目录
                            if SKIP_DIRS.contains(&name.as_ref()) {
                                return false;
                            }

                            // 检查是否匹配 DirName 规则
                            for (dir_name, safety, category) in dirname_clone.iter() {
                                if name.as_ref() == dir_name.as_str() {
                                    // Rust target 目录需要验证父目录有 Cargo.toml
                                    if dir_name == "target" {
                                        let entry_path = e.path();
                                        if let Some(p) = entry_path.parent() {
                                            if !p.join("Cargo.toml").exists() {
                                                continue;
                                            }
                                        }
                                    }

                                    let entry_path = e.path();

                                    if let Ok(mut items) = matched_clone.lock() {
                                        items.push((
                                            entry_path,
                                            *safety,
                                            category.clone(),
                                        ));
                                    }

                                    // 不再深入已匹配的目录
                                    return false;
                                }
                            }
                        }
                    }
                    true
                });
            });

        // 驱动遍历器消费所有条目
        for _ in walker {}

        // 遍历完成后，为匹配的目录计算大小并生成 ScanItem
        let paths = std::sync::Arc::try_unwrap(matched_paths)
            .map(|mutex| mutex.into_inner().unwrap_or_default())
            .unwrap_or_else(|arc| arc.lock().unwrap().clone());
        for (path, safety, category) in paths {
            let size = dir_size(&path);

            reporter.on_event(ProgressEvent::Found {
                category: category.clone(),
                path: path.clone(),
                size,
            });

            let item = ScanItem::new(path, size, safety, category.clone());
            category_map.entry(category).or_default().push(item);
        }

        let result = build_scan_result(category_map, reporter);
        Ok(result)
    }
}

/// 将 category_map 转换为 ScanResult，并报告进度
fn build_scan_result(
    category_map: HashMap<String, Vec<ScanItem>>,
    reporter: &dyn ProgressReporter,
) -> ScanResult {
    let mut categories: Vec<CategoryGroup> = category_map
        .into_iter()
        .map(|(name, items)| {
            let group = CategoryGroup::new(name, items);
            reporter.on_event(ProgressEvent::CategoryDone {
                category: group.name.clone(),
                total_size: group.total_size,
                count: group.file_count,
            });
            group
        })
        .collect();

    // 按名称排序保持稳定输出
    categories.sort_by(|a, b| a.name.cmp(&b.name));

    let result = ScanResult::from_categories(categories);
    reporter.on_event(ProgressEvent::Complete);
    result
}

/// 使用 jwalk 并行计算目录总大小
fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    let walker = jwalk::WalkDir::new(path).skip_hidden(false).follow_links(false);
    let mut total: u64 = 0;

    for entry in walker {
        if let Ok(entry) = entry {
            if !entry.file_type().is_dir() {
                if let Ok(meta) = std::fs::symlink_metadata(entry.path()) {
                    total += meta.len();
                }
            }
        }
    }

    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::{ProgressEvent, ProgressReporter};
    use std::sync::{Arc, Mutex};
    use tempfile::tempdir;

    /// 收集所有进度事件的测试用 reporter
    struct TestReporter {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl TestReporter {
        fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
            let events = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    events: events.clone(),
                },
                events,
            )
        }
    }

    impl ProgressReporter for TestReporter {
        fn on_event(&self, event: ProgressEvent) {
            let tag = match &event {
                ProgressEvent::Scanning { .. } => "Scanning".to_string(),
                ProgressEvent::Found { category, .. } => format!("Found:{}", category),
                ProgressEvent::CategoryDone { category, .. } => {
                    format!("CategoryDone:{}", category)
                }
                ProgressEvent::Complete => "Complete".to_string(),
                ProgressEvent::Error(msg) => format!("Error:{}", msg),
                ProgressEvent::CleaningFile { .. } => "CleaningFile".to_string(),
                ProgressEvent::CleaningDone { .. } => "CleaningDone".to_string(),
            };
            if let Ok(mut evts) = self.events.lock() {
                evts.push(tag);
            }
        }
    }

    #[test]
    fn test_dir_size_sums_files() {
        let tmp = tempdir().unwrap();
        let dir = tmp.path();

        // 创建几个文件
        std::fs::write(dir.join("a.txt"), "hello").unwrap(); // 5 bytes
        std::fs::write(dir.join("b.txt"), "world!").unwrap(); // 6 bytes

        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("c.txt"), "1234567890").unwrap(); // 10 bytes

        let size = dir_size(dir);
        assert_eq!(size, 21, "目录总大小应为 21 字节");
    }

    #[test]
    fn test_dir_size_empty_dir() {
        let tmp = tempdir().unwrap();
        let size = dir_size(tmp.path());
        assert_eq!(size, 0, "空目录大小应为 0");
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let size = dir_size(Path::new("/nonexistent_path_xyz"));
        assert_eq!(size, 0, "不存在的路径大小应为 0");
    }

    #[test]
    fn test_scan_purge_finds_node_modules_and_venv() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建 project_a/node_modules 结构
        let nm = base.join("project_a").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("pkg.js"), "console.log('hi')").unwrap();

        // 创建 project_b/.venv 结构
        let venv = base.join("project_b").join(".venv");
        std::fs::create_dir_all(&venv).unwrap();
        std::fs::write(venv.join("pyvenv.cfg"), "home = /usr").unwrap();

        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(base, &reporter).unwrap();

        // 应找到 node_modules 和 .venv
        let all_paths: Vec<String> = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .map(|i| i.path.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(
            all_paths.contains(&"node_modules".to_string()),
            "应找到 node_modules，实际找到: {:?}",
            all_paths
        );
        assert!(
            all_paths.contains(&".venv".to_string()),
            "应找到 .venv，实际找到: {:?}",
            all_paths
        );
    }

    #[test]
    fn test_scan_purge_empty_dir_returns_empty() {
        let tmp = tempdir().unwrap();
        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(tmp.path(), &reporter).unwrap();

        assert!(
            result.categories.is_empty(),
            "空目录应返回空的扫描结果"
        );
        assert_eq!(result.total_size, 0);
        assert_eq!(result.file_count, 0);
    }

    #[test]
    fn test_scan_purge_reports_progress_events() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建一个 node_modules 目录
        let nm = base.join("myproject").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("index.js"), "module.exports = {}").unwrap();

        let (reporter, events) = TestReporter::new();
        Scanner::scan_purge(base, &reporter).unwrap();

        let evts = events.lock().unwrap();

        assert!(
            evts.contains(&"Scanning".to_string()),
            "应包含 Scanning 事件"
        );
        assert!(
            evts.iter().any(|e| e.starts_with("Found:")),
            "应包含 Found 事件"
        );
        assert!(
            evts.iter().any(|e| e.starts_with("CategoryDone:")),
            "应包含 CategoryDone 事件"
        );
        assert!(
            evts.contains(&"Complete".to_string()),
            "应包含 Complete 事件"
        );
    }

    #[test]
    fn test_scan_purge_does_not_descend_into_matched_dirs() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建 node_modules 内部嵌套 node_modules
        let outer_nm = base.join("proj").join("node_modules");
        let inner_nm = outer_nm.join("some_pkg").join("node_modules");
        std::fs::create_dir_all(&inner_nm).unwrap();
        std::fs::write(inner_nm.join("nested.js"), "x").unwrap();

        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(base, &reporter).unwrap();

        // 只应找到外层 node_modules，不应找到内层
        let nm_items: Vec<&ScanItem> = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| {
                i.path
                    .file_name()
                    .map(|n| n == "node_modules")
                    .unwrap_or(false)
            })
            .collect();

        assert_eq!(
            nm_items.len(),
            1,
            "应只找到 1 个 node_modules（外层），实际找到 {}",
            nm_items.len()
        );

        // 但其大小应包含内部嵌套的文件
        assert!(
            nm_items[0].size > 0,
            "node_modules 大小应 > 0（包含嵌套文件）"
        );
    }

    #[test]
    fn test_symlinks_not_followed() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建一个实际目录和文件
        let real_dir = base.join("real");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::write(real_dir.join("data.bin"), vec![0u8; 1000]).unwrap();

        // 创建一个伪 node_modules 内有符号链接指向 real 目录
        let nm = base.join("project").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("small.js"), "x").unwrap();

        // 创建符号链接 node_modules/linked -> real
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_dir, nm.join("linked")).unwrap();
        }

        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(base, &reporter).unwrap();

        // 找到 node_modules
        let nm_item = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .find(|i| {
                i.path
                    .file_name()
                    .map(|n| n == "node_modules")
                    .unwrap_or(false)
            });

        assert!(nm_item.is_some(), "应找到 node_modules");

        let nm_size = nm_item.unwrap().size;
        // node_modules 大小不应包含 real 目录的 1000 字节（因为不跟随符号链接）
        assert!(
            nm_size < 1000,
            "不应跟随符号链接计算大小，实际大小: {}",
            nm_size
        );
    }

    #[test]
    fn test_rust_target_requires_cargo_toml() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 项目 A：有 Cargo.toml，target 应被匹配
        let proj_a = base.join("rust_proj");
        std::fs::create_dir_all(proj_a.join("target")).unwrap();
        std::fs::write(proj_a.join("target").join("output.o"), "binary").unwrap();
        std::fs::write(proj_a.join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();

        // 项目 B：没有 Cargo.toml，target 不应被匹配
        let proj_b = base.join("other_proj");
        std::fs::create_dir_all(proj_b.join("target")).unwrap();
        std::fs::write(proj_b.join("target").join("output.o"), "binary").unwrap();

        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(base, &reporter).unwrap();

        let target_items: Vec<&ScanItem> = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.path.file_name().map(|n| n == "target").unwrap_or(false))
            .collect();

        assert_eq!(
            target_items.len(),
            1,
            "应只匹配含 Cargo.toml 的 target 目录"
        );
        assert!(
            target_items[0].path.starts_with(&proj_a),
            "匹配的 target 应在 rust_proj 下"
        );
    }
}
