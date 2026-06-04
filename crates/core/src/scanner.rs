use crate::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
use crate::progress::{ProgressEvent, ProgressReporter};
use crate::rules::{clean_rules, purge_rules, CleanRule, PathPattern};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// 需要在遍历时跳过的目录名（通用）
const SKIP_DIRS: &[&str] = &[".git", ".Spotlight-V100", ".fseventsd"];

/// purge 模式额外跳过的目录（不可能包含开发产物的大目录）
const PURGE_SKIP_DIRS: &[&str] = &[
    ".git",
    ".Spotlight-V100",
    ".fseventsd",
    "Library",
    "Applications",
    ".Trash",
    "Pictures",
    "Music",
    "Movies",
];

#[cfg(target_os = "macos")]
const WALK_THREADS: usize = 3;
#[cfg(not(target_os = "macos"))]
const WALK_THREADS: usize = 0;

/// 带预取 metadata 的 walker 类型别名：client_state 存储文件大小 Option<u64>
pub type MetaWalkDir = jwalk::WalkDirGeneric<((), Option<u64>)>;

/// 创建带正确并行度配置的 walker（返回类型支持 client_state 预取）
///
/// 注意：此函数不设置 `process_read_dir`，调用方需自行设置并在回调中调用
/// `prefetch_metadata` 来预取文件大小。这是因为 `process_read_dir` 是替换型 API，
/// 后调用会覆盖前调用。
pub fn create_walker(path: &Path) -> MetaWalkDir {
    let parallelism = if WALK_THREADS == 0 {
        jwalk::Parallelism::RayonDefaultPool { busy_timeout: std::time::Duration::from_secs(1) }
    } else {
        jwalk::Parallelism::RayonNewPool(WALK_THREADS)
    };
    MetaWalkDir::new(path)
        .skip_hidden(false)
        .follow_links(false)
        .parallelism(parallelism)
}

/// 在 `process_read_dir` 回调中预取每个条目的 metadata，将文件大小存入 `client_state`。
/// 在 rayon 工作线程上执行，消费端可零成本读取 `entry.client_state`。
pub fn prefetch_metadata(
    children: &mut Vec<jwalk::Result<jwalk::DirEntry<((), Option<u64>)>>>,
) {
    for entry in children.iter_mut() {
        if let Ok(dir_entry) = entry {
            if !dir_entry.file_type.is_dir() {
                dir_entry.client_state =
                    dir_entry.metadata().map(|m| m.len()).ok();
            }
        }
    }
}

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

    /// 按 Exact 规则扫描：合并重叠路径后遍历，每个文件只计入最具体的匹配规则
    fn scan_with_rules(
        rules: &[CleanRule],
        reporter: &dyn ProgressReporter,
    ) -> anyhow::Result<ScanResult> {
        let mut category_map: HashMap<String, Vec<ScanItem>> = HashMap::new();

        // 收集所有 (path, safety, category) 并按路径排序（短路径优先）
        let mut all_paths: Vec<(PathBuf, SafetyLevel, String)> = Vec::new();
        for rule in rules {
            for pattern in &rule.patterns {
                if let PathPattern::Exact(base) = pattern {
                    if base.exists() {
                        all_paths.push((base.clone(), rule.safety, rule.category.clone()));
                    }
                }
            }
        }
        all_paths.sort_by(|a, b| a.0.cmp(&b.0));

        // 识别根路径（不被其他路径包含的路径）
        // 对于被包含的路径，记录为子规则
        struct RootEntry {
            path: PathBuf,
            safety: SafetyLevel,
            category: String,
            children: Vec<(PathBuf, SafetyLevel, String)>,
        }

        let mut roots: Vec<RootEntry> = Vec::new();
        for (path, safety, category) in &all_paths {
            let is_child = roots.iter_mut().any(|root| {
                if path.starts_with(&root.path) && path != &root.path {
                    root.children.push((path.clone(), *safety, category.clone()));
                    true
                } else {
                    false
                }
            });
            if !is_child {
                roots.push(RootEntry {
                    path: path.clone(),
                    safety: *safety,
                    category: category.clone(),
                    children: Vec::new(),
                });
            }
        }

        let root_count = roots.len();
        // 遍历每个根路径
        for (root_idx, root) in roots.iter().enumerate() {
            if reporter.is_cancelled() {
                break;
            }

            reporter.on_event(ProgressEvent::RuleProgress {
                current: root_idx + 1,
                total: root_count,
                name: root.category.clone(),
            });
            reporter.on_event(ProgressEvent::Scanning {
                path: root.path.clone(),
            });

            let walker = create_walker(&root.path)
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
                    prefetch_metadata(children);
                });

            let mut batch_count: usize = 0;
            // 每个规则的大小累加器：(category, size)
            let mut size_by_category: HashMap<String, u64> = HashMap::new();

            for entry in walker {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                if entry.file_type().is_dir() {
                    continue;
                }

                let size = entry.client_state.unwrap_or(0);
                let path = entry.path();

                batch_count += 1;

                if batch_count.is_multiple_of(200) {
                    if reporter.is_cancelled() {
                        break;
                    }
                    reporter.on_event(ProgressEvent::Scanning {
                        path: path.clone(),
                    });
                }

                // 最长前缀匹配：文件归入最具体的子规则，否则归入根规则
                let (safety, category) = root
                    .children
                    .iter()
                    .rev()
                    .find(|(child_path, _, _)| path.starts_with(child_path))
                    .map(|(_, s, c)| (*s, c.clone()))
                    .unwrap_or((root.safety, root.category.clone()));

                *size_by_category.entry(category.clone()).or_insert(0) += size;

                let item = ScanItem::new(path, size, safety, category);
                category_map
                    .entry(item.category.clone())
                    .or_default()
                    .push(item);
            }

            // 为根路径下的每个 category 发送 Found 事件
            for (category, total_size) in &size_by_category {
                let safety = root
                    .children
                    .iter()
                    .find(|(_, _, c)| c == category)
                    .map(|(_, s, _)| *s)
                    .unwrap_or(root.safety);
                reporter.on_event(ProgressEvent::Found {
                    category: category.clone(),
                    path: root.path.clone(),
                    size: *total_size,
                    safety,
                });
            }
        }

        let result = build_scan_result(category_map, reporter);
        Ok(result)
    }

    /// 按 DirName 规则扫描：单遍遍历，就地累加匹配目录的大小
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

        // Exact 规则（如 DerivedData）单独处理——并行计算各路径大小
        let exact_entries: Vec<(PathBuf, SafetyLevel, String)> = rules
            .iter()
            .flat_map(|rule| {
                rule.patterns.iter().filter_map(move |p| {
                    if let PathPattern::Exact(exact_path) = p {
                        if exact_path.exists() && exact_path.starts_with(base_path) {
                            return Some((exact_path.clone(), rule.safety, rule.category.clone()));
                        }
                    }
                    None
                })
            })
            .collect();

        let dir_size_pool = build_dir_size_pool();

        let exact_results: Vec<(PathBuf, u64, SafetyLevel, String)> =
            dir_size_pool.install(|| {
                exact_entries
                    .par_iter()
                    .map(|(path, safety, category)| {
                        let size = dir_size(path);
                        (path.clone(), size, *safety, category.clone())
                    })
                    .collect()
            });

        for (path, size, safety, category) in exact_results {
            reporter.on_event(ProgressEvent::Found {
                category: category.clone(),
                path: path.clone(),
                size,
                safety,
            });
            let item = ScanItem::new(path, size, safety, category.clone());
            category_map.entry(category).or_default().push(item);
        }

        // 剪枝遍历：匹配到目录名后立即剪枝（不进入子树），收集匹配路径
        // 遍历完成后再并行计算各目录大小
        let matched_dirs: Arc<Mutex<Vec<(PathBuf, SafetyLevel, String)>>> =
            Arc::new(Mutex::new(Vec::new()));

        let dirname_rules_arc = Arc::new(dirname_rules);
        let matched_clone = matched_dirs.clone();
        let dirname_clone = dirname_rules_arc.clone();

        let walker = create_walker(base_path)
            .process_read_dir(move |_depth, _path, _state, children| {
                children.retain(|entry| {
                    if let Ok(ref e) = entry {
                        if e.file_type().is_dir() {
                            let name = e.file_name().to_string_lossy();

                            if PURGE_SKIP_DIRS.contains(&name.as_ref()) {
                                return false;
                            }

                            let entry_path = e.path();

                            for (dir_name, safety, category) in dirname_clone.iter() {
                                if name.as_ref() == dir_name.as_str() {
                                    if dir_name == "target" {
                                        if let Some(p) = entry_path.parent() {
                                            if !p.join("Cargo.toml").exists() {
                                                continue;
                                            }
                                        }
                                    }
                                    if let Ok(mut dirs) = matched_clone.lock() {
                                        dirs.push((
                                            entry_path,
                                            *safety,
                                            category.clone(),
                                        ));
                                    }
                                    return false; // 剪枝：不进入匹配的目录子树
                                }
                            }
                        }
                    }
                    true
                });
            });

        // 快速遍历（剪枝后只触碰非匹配目录）
        for entry in walker {
            if reporter.is_cancelled() {
                break;
            }
            let _ = entry;
        }

        // 并行计算各匹配目录的大小
        let dirs = Arc::try_unwrap(matched_dirs)
            .map(|mutex| mutex.into_inner().unwrap_or_default())
            .unwrap_or_else(|arc| arc.lock().unwrap().clone());

        let dir_sizes: Vec<(PathBuf, u64, SafetyLevel, String)> =
            dir_size_pool.install(|| {
                dirs.par_iter()
                    .map(|(path, safety, category)| {
                        let size = dir_size(path);
                        (path.clone(), size, *safety, category.clone())
                    })
                    .collect()
            });

        for (path, size, safety, category) in dir_sizes {
            reporter.on_event(ProgressEvent::Found {
                category: category.clone(),
                path: path.clone(),
                size,
                safety,
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

    let walker = create_walker(path)
        .process_read_dir(|_depth, _path, _state, children| {
            prefetch_metadata(children);
        });
    let mut total: u64 = 0;

    for entry in walker.into_iter().flatten() {
        if !entry.file_type().is_dir() {
            total += entry.client_state.unwrap_or(0);
        }
    }

    total
}

#[cfg(target_os = "macos")]
extern "C" {
    fn setiopolicy_np(iotype: i32, scope: i32, policy: i32) -> i32;
}

/// 构建 dir_size 专用线程池：4 线程 + macOS I/O 优先级降级
fn build_dir_size_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .thread_name(|i| format!("mc-dir-size-{i}"))
        .start_handler(|_| {
            #[cfg(target_os = "macos")]
            unsafe {
                // IOPOL_TYPE_DISK=0, IOPOL_SCOPE_THREAD=1, IOPOL_UTILITY=4
                setiopolicy_np(0, 1, 4);
            }
        })
        .build()
        .expect("failed to build dir_size thread pool")
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
                ProgressEvent::Found { ref category, .. } => format!("Found:{}", category),
                ProgressEvent::CategoryDone { category, .. } => {
                    format!("CategoryDone:{}", category)
                }
                ProgressEvent::RuleProgress { current, total, .. } => {
                    format!("RuleProgress:{}/{}", current, total)
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
