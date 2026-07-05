use crate::models::{CategoryGroup, SafetyLevel, ScanItem, ScanResult};
use crate::progress::{ProgressEvent, ProgressReporter};
use crate::rules::{
    clean_rules, matches_root_markers, purge_rules, CleanRule, PathPattern, RootMarker,
};
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

/// 带预取 metadata 的 walker `类型别名：client_state` 存储文件大小 Option<u64>
pub type MetaWalkDir = jwalk::WalkDirGeneric<((), Option<u64>)>;

/// 创建带正确并行度配置的 walker（返回类型支持 `client_state` 预取）
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
    for dir_entry in children.iter_mut().flatten() {
        if !dir_entry.file_type.is_dir() {
            dir_entry.client_state =
                dir_entry.metadata().map(|m| m.len()).ok();
        }
    }
}

/// 规则派生的每项元数据，随扫描在各中间集合间流转（避免多元组膨胀）。
#[derive(Clone)]
struct Meta {
    safety: SafetyLevel,
    category: String,
    impact: String,
    recovery: String,
    preselect: bool,
}

impl Meta {
    fn from_rule(rule: &CleanRule) -> Self {
        Self {
            safety: rule.safety,
            category: rule.category.clone(),
            impact: rule.impact.clone(),
            recovery: rule.recovery.clone(),
            preselect: rule.preselect,
        }
    }

    /// 兜底元数据（分类名未在映射中找到时使用；正常不应触发）。
    fn fallback(category: String) -> Self {
        Self {
            safety: SafetyLevel::Safe,
            category,
            impact: String::new(),
            recovery: String::new(),
            preselect: true,
        }
    }

    fn into_item(self, path: PathBuf, size: u64) -> ScanItem {
        ScanItem::new(path, size, self.safety, self.category)
            .with_evidence(self.impact, self.recovery)
            .with_preselect(self.preselect)
    }

    fn found(&self, path: PathBuf, size: u64) -> ProgressEvent {
        ProgressEvent::Found {
            category: self.category.clone(),
            path,
            size,
            safety: self.safety,
            impact: self.impact.clone(),
            recovery: self.recovery.clone(),
            preselect: self.preselect,
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

        // 收集所有 (path, meta) 并按路径排序（短路径优先）
        let mut all_paths: Vec<(PathBuf, Meta)> = Vec::new();
        for rule in rules {
            for pattern in &rule.patterns {
                if let PathPattern::Exact(base) = pattern {
                    if base.exists() {
                        all_paths.push((base.clone(), Meta::from_rule(rule)));
                    }
                }
            }
        }
        all_paths.sort_by(|a, b| a.0.cmp(&b.0));

        // 识别根路径（不被其他路径包含的路径）
        // 对于被包含的路径，记录为子规则
        struct RootEntry {
            path: PathBuf,
            meta: Meta,
            children: Vec<(PathBuf, Meta)>,
        }

        let mut roots: Vec<RootEntry> = Vec::new();
        for (path, meta) in &all_paths {
            let is_child = roots.iter_mut().any(|root| {
                if path.starts_with(&root.path) && path != &root.path {
                    root.children.push((path.clone(), meta.clone()));
                    true
                } else {
                    false
                }
            });
            if !is_child {
                roots.push(RootEntry {
                    path: path.clone(),
                    meta: meta.clone(),
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
                name: root.meta.category.clone(),
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

            // 预计算本根下各 category 的 safety，用于流式上报 Found。
            // 按 category 名索引依赖"同名分类 safety 唯一"这一不变式（clean/purge 规则表当前成立）；
            // 若未来某分类在同根下映射到两种 safety，debug 构建会在此断言失败以提醒改用更精确的键。
            let mut cat_meta: HashMap<String, Meta> = HashMap::new();
            cat_meta.insert(root.meta.category.clone(), root.meta.clone());
            for (_, m) in &root.children {
                debug_assert!(
                    cat_meta
                        .get(&m.category)
                        .is_none_or(|prev| prev.safety == m.safety),
                    "分类 {:?} 在同根下出现两种 safety，按名索引会丢失其一",
                    m.category
                );
                cat_meta.insert(m.category.clone(), m.clone());
            }

            let mut batch_count: usize = 0;
            // 每个规则的大小累加器：(category, 累计 size)
            let mut size_by_category: HashMap<String, u64> = HashMap::new();
            // 已流式上报的累计 size，用于计算增量（delta）
            let mut emitted_by_category: HashMap<String, u64> = HashMap::new();

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

                // 最长前缀匹配：文件归入最具体的子规则，否则归入根规则
                let meta = root
                    .children
                    .iter()
                    .rev()
                    .find(|(child_path, _)| path.starts_with(child_path))
                    .map_or_else(|| root.meta.clone(), |(_, m)| m.clone());

                *size_by_category.entry(meta.category.clone()).or_insert(0) += size;

                let item = meta.into_item(path.clone(), size);
                category_map
                    .entry(item.category.clone())
                    .or_default()
                    .push(item);

                // 每 200 个文件流式上报一次各分类的增量，让 TUI 列表边扫边填充，
                // 而不是等整个根目录遍历完才一次性出现（Clean 卡顿感的根因）。
                if batch_count.is_multiple_of(200) {
                    if reporter.is_cancelled() {
                        break;
                    }
                    reporter.on_event(ProgressEvent::Scanning { path });
                    flush_category_deltas(
                        reporter,
                        &root.path,
                        &size_by_category,
                        &mut emitted_by_category,
                        &cat_meta,
                    );
                }
            }

            // 收尾：上报剩余增量（不足 200 的尾巴）
            flush_category_deltas(
                reporter,
                &root.path,
                &size_by_category,
                &mut emitted_by_category,
                &cat_meta,
            );
        }

        let result = build_scan_result(category_map, reporter);
        Ok(result)
    }

    /// 按 `DirName` 规则扫描：单遍遍历，就地累加匹配目录的大小
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

        let dirname_rules: Vec<(String, Vec<RootMarker>, Meta)> = rules
            .iter()
            .flat_map(|rule| {
                let meta = Meta::from_rule(rule);
                let markers = rule.root_markers.clone();
                rule.patterns.iter().filter_map(move |p| {
                    if let PathPattern::DirName(name) = p {
                        Some((name.clone(), markers.clone(), meta.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect();

        // Exact 规则（如 DerivedData）单独处理——并行计算各路径大小
        let exact_entries: Vec<(PathBuf, Meta)> = rules
            .iter()
            .flat_map(|rule| {
                let meta = Meta::from_rule(rule);
                rule.patterns.iter().filter_map(move |p| {
                    if let PathPattern::Exact(exact_path) = p {
                        if exact_path.exists() && exact_path.starts_with(base_path) {
                            return Some((exact_path.clone(), meta.clone()));
                        }
                    }
                    None
                })
            })
            .collect();

        let dir_size_pool = build_dir_size_pool();

        // Exact 路径（如 Xcode DerivedData）体积可达数十 GB，其 dir_size 单个就要数秒。
        // 在并行 map 内：开算前报 Scanning(当前路径)、算完报 Found，避免这段静默造成
        // "扫描中: ~ + 长时间无反应"的冻结感。
        let exact_results: Vec<(PathBuf, u64, Meta)> =
            dir_size_pool.install(|| {
                exact_entries
                    .par_iter()
                    .map(|(path, meta)| {
                        if reporter.is_cancelled() {
                            return (path.clone(), 0, meta.clone());
                        }
                        reporter.on_event(ProgressEvent::Scanning { path: path.clone() });
                        let size = dir_size(path, reporter);
                        reporter.on_event(meta.found(path.clone(), size));
                        (path.clone(), size, meta.clone())
                    })
                    .collect()
            });

        for (path, size, meta) in exact_results {
            let category = meta.category.clone();
            let item = meta.into_item(path, size);
            category_map.entry(category).or_default().push(item);
        }

        // 剪枝遍历：匹配到目录名后立即剪枝（不进入子树），收集匹配路径
        // 遍历完成后再并行计算各目录大小
        let matched_dirs: Arc<Mutex<Vec<(PathBuf, Meta)>>> =
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

                            for (dir_name, markers, meta) in dirname_clone.iter() {
                                if name.as_ref() == dir_name.as_str() {
                                    // 项目根守卫：不满足标记则跳过此规则（如无 Cargo.toml 的 target、
                                    // 无 package.json 的 build），消除按目录名匹配的误报。
                                    if !matches_root_markers(markers, &entry_path) {
                                        continue;
                                    }
                                    if let Ok(mut dirs) = matched_clone.lock() {
                                        dirs.push((entry_path, meta.clone()));
                                    }
                                    return false; // 剪枝：不进入匹配的目录子树
                                }
                            }
                        }
                    }
                    true
                });
            });

        // 快速遍历（剪枝后只触碰非匹配目录）。
        // 遍历整棵树可能耗时数秒——期间必须周期性上报当前目录，否则界面停在
        // "已扫描 0 | 当前:空 + spinner" 看起来像卡死（对齐 Analyze 的实时反馈）。
        let mut walked: u64 = 0;
        for entry in walker {
            if reporter.is_cancelled() {
                break;
            }
            if let Ok(e) = entry {
                if e.file_type().is_dir() {
                    walked += 1;
                    if walked.is_multiple_of(48) {
                        reporter.on_event(ProgressEvent::Scanning { path: e.path() });
                    }
                }
            }
        }

        // 并行计算各匹配目录的大小。这是 Purge 最耗时的相位（大目录可达上百秒）——
        // 因此在并行 map 内**边算边流式 emit Found**，让 TUI 逐个填充列表、found 计数
        // 实时增长，而非静默上百秒后一次性爆出（对齐 Analyze 的实时反馈）。
        let dirs = Arc::try_unwrap(matched_dirs).map_or_else(|arc| arc.lock().unwrap().clone(), |mutex| mutex.into_inner().unwrap_or_default());

        let dir_sizes: Vec<(PathBuf, u64, Meta)> =
            dir_size_pool.install(|| {
                dirs.par_iter()
                    .map(|(path, meta)| {
                        if reporter.is_cancelled() {
                            return (path.clone(), 0, meta.clone());
                        }
                        let size = dir_size(path, reporter);
                        // 每算完一个目录立刻上报，供 TUI 增量渲染
                        reporter.on_event(meta.found(path.clone(), size));
                        (path.clone(), size, meta.clone())
                    })
                    .collect()
            });

        // 汇总用于返回值（CLI 路径）；Found 已在并行阶段发过，此处不再重复 emit
        for (path, size, meta) in dir_sizes {
            let category = meta.category.clone();
            let item = meta.into_item(path, size);
            category_map.entry(category).or_default().push(item);
        }

        let result = build_scan_result(category_map, reporter);
        Ok(result)
    }
}

/// 将 `category_map` 转换为 ScanResult，并报告进度
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

/// 上报各分类相对上次已报 size 的增量（delta），驱动 TUI 列表边扫边填充。
/// TUI 侧对同一 `(category, path)` 的重复 `Found` 会合并累加，故此处发 delta 即可，
/// 累加后各分类聚合项的大小与最终一次性上报完全一致。
fn flush_category_deltas(
    reporter: &dyn ProgressReporter,
    root_path: &Path,
    size_by_category: &HashMap<String, u64>,
    emitted: &mut HashMap<String, u64>,
    cat_meta: &HashMap<String, Meta>,
) {
    for (category, &cum) in size_by_category {
        let last = emitted.get(category).copied();
        // 已上报过且无新增（cum 单调不减，等于即无增量）则跳过；但**首次**出现即使当前
        // 累计为 0 也上报一条 size=0 的 Found 建项，让"全零大小分类"也能出现在 TUI（与 CLI 一致）。
        if last == Some(cum) {
            continue;
        }
        let delta = cum - last.unwrap_or(0);
        let meta = cat_meta
            .get(category)
            .cloned()
            .unwrap_or_else(|| Meta::fallback(category.clone()));
        reporter.on_event(meta.found(root_path.to_path_buf(), delta));
        emitted.insert(category.clone(), cum);
    }
}

/// 使用 jwalk 并行计算目录总大小。
///
/// 单个大目录（如 Docker/DerivedData 数十 GB）的遍历可达数秒~上百秒，故在消费循环里
/// 每 1024 个 entry 检查一次取消：用户取消后尽快中止，不再空耗 CPU/IO 与新扫描抢占磁盘。
fn dir_size(path: &Path, reporter: &dyn ProgressReporter) -> u64 {
    if !path.exists() {
        return 0;
    }

    let walker = create_walker(path)
        .process_read_dir(|_depth, _path, _state, children| {
            prefetch_metadata(children);
        });
    let mut total: u64 = 0;
    let mut seen: u64 = 0;

    for entry in walker.into_iter().flatten() {
        seen += 1;
        if seen.is_multiple_of(1024) && reporter.is_cancelled() {
            break;
        }
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

/// 构建 `dir_size` 专用线程池：4 线程 + macOS I/O 优先级降级
fn build_dir_size_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .thread_name(|i| format!("mc-dir-size-{i}"))
        .start_handler(|_| {
            #[cfg(target_os = "macos")]
            #[allow(unsafe_code)]
            unsafe {
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
                ProgressEvent::Found { ref category, .. } => format!("Found:{category}"),
                ProgressEvent::CategoryDone { category, .. } => {
                    format!("CategoryDone:{category}")
                }
                ProgressEvent::RuleProgress { current, total, .. } => {
                    format!("RuleProgress:{current}/{total}")
                }
                ProgressEvent::Complete => "Complete".to_string(),
                ProgressEvent::Error(msg) => format!("Error:{msg}"),
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

        let size = dir_size(dir, &crate::progress::NoopReporter);
        assert_eq!(size, 21, "目录总大小应为 21 字节");
    }

    #[test]
    fn test_dir_size_empty_dir() {
        let tmp = tempdir().unwrap();
        let size = dir_size(tmp.path(), &crate::progress::NoopReporter);
        assert_eq!(size, 0, "空目录大小应为 0");
    }

    #[test]
    fn test_dir_size_nonexistent() {
        let size = dir_size(Path::new("/nonexistent_path_xyz"), &crate::progress::NoopReporter);
        assert_eq!(size, 0, "不存在的路径大小应为 0");
    }

    #[test]
    fn test_scan_purge_finds_node_modules_and_venv() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建 project_a/node_modules 结构（含 sibling package.json 守卫标记）
        let nm = base.join("project_a").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("pkg.js"), "console.log('hi')").unwrap();
        std::fs::write(base.join("project_a").join("package.json"), "{}").unwrap();

        // 创建 project_b/.venv 结构（含 inside pyvenv.cfg 守卫标记）
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
            "应找到 node_modules，实际找到: {all_paths:?}"
        );
        assert!(
            all_paths.contains(&".venv".to_string()),
            "应找到 .venv，实际找到: {all_paths:?}"
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

        // 创建一个 node_modules 目录（含 sibling package.json 守卫标记）
        let nm = base.join("myproject").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("index.js"), "module.exports = {}").unwrap();
        std::fs::write(base.join("myproject").join("package.json"), "{}").unwrap();

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

    /// 累加每个分类收到的 Found size（用于验证流式 delta 的求和正确性）
    struct SizeReporter {
        found: Arc<Mutex<HashMap<String, u64>>>,
        found_events: Arc<Mutex<usize>>,
    }

    impl ProgressReporter for SizeReporter {
        fn on_event(&self, event: ProgressEvent) {
            if let ProgressEvent::Found { category, size, .. } = event {
                *self.found.lock().unwrap().entry(category).or_insert(0) += size;
                *self.found_events.lock().unwrap() += 1;
            }
        }
    }

    #[test]
    fn scan_clean_streamed_found_deltas_sum_to_true_total() {
        // 超过 200 个文件以触发遍历途中的增量 flush（而非仅收尾一次），
        // 验证流式 delta 求和后与目录真实总大小完全一致、不重复计数。
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("cache");
        std::fs::create_dir_all(&root).unwrap();
        let mut expected: u64 = 0;
        for i in 0..250u32 {
            let content = format!("file-{i}-payload");
            expected += content.len() as u64;
            std::fs::write(root.join(format!("f{i}.bin")), content).unwrap();
        }

        let rules = vec![CleanRule {
            name: "test".into(),
            description: String::new(),
            patterns: vec![PathPattern::Exact(root.clone())],
            safety: SafetyLevel::Safe,
            category: "测试缓存".into(),
            impact: String::new(),
            recovery: String::new(),
            root_markers: Vec::new(),
            preselect: true,
        }];

        let found = Arc::new(Mutex::new(HashMap::new()));
        let found_events = Arc::new(Mutex::new(0usize));
        let reporter = SizeReporter {
            found: found.clone(),
            found_events: found_events.clone(),
        };

        let result = Scanner::scan_with_rules(&rules, &reporter).unwrap();

        // 多次增量 flush：Found 事件数应 > 1（否则未真正流式）
        assert!(
            *found_events.lock().unwrap() > 1,
            "250 个文件应触发多次流式 Found，实际次数: {}",
            *found_events.lock().unwrap()
        );
        // 流式 delta 求和 == 真实总大小
        assert_eq!(
            found.lock().unwrap().get("测试缓存").copied().unwrap_or(0),
            expected,
            "流式 Found delta 求和应等于目录真实总大小"
        );
        // 返回值（CLI 路径）总大小同样正确
        assert_eq!(result.total_size, expected);
    }

    #[test]
    fn scan_clean_streams_multiple_categories_under_one_root() {
        // 根规则 + 最长前缀子规则：两个分类都以 path=root.path 流式 emit Found。
        // 验证 TUI 侧按 (category, path) 合并时不会把不同分类混淆，各自求和独立正确。
        let tmp = tempdir().unwrap();
        let root = tmp.path().join("cache");
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        // 直接位于 root 下的文件归"根缓存"；位于 sub 下的归"子缓存"。
        // 总数 >200 触发遍历途中的增量 flush。
        let mut root_expected: u64 = 0;
        for i in 0..150u32 {
            let content = format!("root-{i}-xx");
            root_expected += content.len() as u64;
            std::fs::write(root.join(format!("r{i}.bin")), content).unwrap();
        }
        let mut sub_expected: u64 = 0;
        for i in 0..100u32 {
            let content = format!("sub-{i}-payload-longer");
            sub_expected += content.len() as u64;
            std::fs::write(sub.join(format!("s{i}.bin")), content).unwrap();
        }

        let rules = vec![
            CleanRule {
                name: "root".into(),
                description: String::new(),
                patterns: vec![PathPattern::Exact(root.clone())],
                safety: SafetyLevel::Safe,
                category: "根缓存".into(),
                impact: String::new(),
                recovery: String::new(),
                root_markers: Vec::new(),
                preselect: true,
            },
            CleanRule {
                name: "sub".into(),
                description: String::new(),
                patterns: vec![PathPattern::Exact(sub.clone())],
                safety: SafetyLevel::Moderate,
                category: "子缓存".into(),
                impact: String::new(),
                recovery: String::new(),
                root_markers: Vec::new(),
                preselect: true,
            },
        ];

        let found = Arc::new(Mutex::new(HashMap::new()));
        let found_events = Arc::new(Mutex::new(0usize));
        let reporter = SizeReporter {
            found: found.clone(),
            found_events: found_events.clone(),
        };

        let result = Scanner::scan_with_rules(&rules, &reporter).unwrap();

        let found = found.lock().unwrap();
        assert_eq!(
            found.get("根缓存").copied().unwrap_or(0),
            root_expected,
            "根分类流式 delta 求和应等于其真实大小（不被子分类污染）"
        );
        assert_eq!(
            found.get("子缓存").copied().unwrap_or(0),
            sub_expected,
            "子分类流式 delta 求和应等于其真实大小（最长前缀归类）"
        );
        assert_eq!(result.total_size, root_expected + sub_expected);
    }

    #[test]
    fn test_scan_purge_does_not_descend_into_matched_dirs() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // 创建 node_modules 内部嵌套 node_modules（proj 含 package.json 守卫标记）
        let outer_nm = base.join("proj").join("node_modules");
        let inner_nm = outer_nm.join("some_pkg").join("node_modules");
        std::fs::create_dir_all(&inner_nm).unwrap();
        std::fs::write(inner_nm.join("nested.js"), "x").unwrap();
        std::fs::write(base.join("proj").join("package.json"), "{}").unwrap();

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
                    .is_some_and(|n| n == "node_modules")
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

        // 创建一个伪 node_modules 内有符号链接指向 real 目录（project 含 package.json 守卫标记）
        let nm = base.join("project").join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("small.js"), "x").unwrap();
        std::fs::write(base.join("project").join("package.json"), "{}").unwrap();

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
                    .is_some_and(|n| n == "node_modules")
            });

        assert!(nm_item.is_some(), "应找到 node_modules");

        let nm_size = nm_item.unwrap().size;
        // node_modules 大小不应包含 real 目录的 1000 字节（因为不跟随符号链接）
        assert!(
            nm_size < 1000,
            "不应跟随符号链接计算大小，实际大小: {nm_size}"
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
            .filter(|i| i.path.file_name().is_some_and(|n| n == "target"))
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

    #[test]
    fn test_dirname_root_guards_sibling_and_inside() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // build：sibling package.json 才匹配（防 electron-builder build/ 等误删）
        let js = base.join("js_proj");
        std::fs::create_dir_all(js.join("build")).unwrap();
        std::fs::write(js.join("build").join("a.js"), "x").unwrap();
        std::fs::write(js.join("package.json"), "{}").unwrap();
        let data = base.join("data_dir"); // 无 package.json 的 build 不应匹配
        std::fs::create_dir_all(data.join("build")).unwrap();
        std::fs::write(data.join("build").join("photo.raw"), "x").unwrap();

        // venv：inside pyvenv.cfg 才匹配
        let py = base.join("py_proj");
        std::fs::create_dir_all(py.join("venv")).unwrap();
        std::fs::write(py.join("venv").join("pyvenv.cfg"), "home = /usr").unwrap();
        let py2 = base.join("py_proj2"); // 无 pyvenv.cfg 的 venv 不应匹配
        std::fs::create_dir_all(py2.join("venv")).unwrap();
        std::fs::write(py2.join("venv").join("note.txt"), "x").unwrap();

        let (reporter, _events) = TestReporter::new();
        let result = Scanner::scan_purge(base, &reporter).unwrap();
        let matched: Vec<&PathBuf> = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .map(|i| &i.path)
            .collect();

        let build_hits: Vec<_> = matched.iter().filter(|p| p.ends_with("build")).collect();
        assert_eq!(build_hits.len(), 1, "只有含 package.json 的 build 应匹配");
        assert!(build_hits[0].starts_with(&js));

        let venv_hits: Vec<_> = matched.iter().filter(|p| p.ends_with("venv")).collect();
        assert_eq!(venv_hits.len(), 1, "只有含 pyvenv.cfg 的 venv 应匹配");
        assert!(venv_hits[0].starts_with(&py));
    }
}
