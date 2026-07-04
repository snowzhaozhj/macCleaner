use mc_core::models::{CleanReport, DirNode, SafetyLevel, ScanResult};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// TUI 应用状态机
#[derive(Debug)]
pub enum AppState {
    /// 主菜单
    Menu,
    /// 正在扫描
    Scanning {
        progress_text: String,
        found_count: usize,
        found_size: u64,
        rule_current: usize,
        rule_total: usize,
        rule_name: String,
    },
    /// 扫描结果展示
    Results,
    /// 正在清理
    Cleaning {
        progress_text: String,
    },
    /// 清理完成
    Done {
        message: String,
    },
    /// 磁盘分析浏览器
    Analyzing {
        /// 完整缓存树根节点
        tree_root: Arc<DirNode>,
        /// 导航路径：每个元素是 children 中的索引
        nav_path: Vec<usize>,
        /// 当前选中行
        cursor: usize,
        /// 每层的 cursor 位置缓存（用于 Backspace 恢复）
        cursor_stack: Vec<usize>,
    },
    /// 磁盘分析进行中（增量构建 + 可导航）
    AnalyzingLive {
        tree_root: DirNode,           // owned，非 Arc，正在增量构建
        nav_path: Vec<usize>,
        cursor: usize,
        cursor_stack: Vec<usize>,
        file_count: u64,              // 已发现的文件总数
        total_size: u64,              // 已累计的字节总量
        user_navigated: bool,         // 用户是否已手动导航（true 后 cursor 不再自动跟随）
    },
    /// 正在排序（finalize 在后台线程执行）。
    /// 排序后进入 Analyzing 恒从根开始（`nav_path`/`cursor` 归零），无需携带任何导航状态。
    Sorting,
}

/// 当前激活的命令
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveCommand {
    Clean,
    Uninstall,
    Analyze,
    Purge,
}

/// 应用主结构体
pub struct App {
    pub state: AppState,
    pub active_command: Option<ActiveCommand>,
    pub scan_result: Option<ScanResult>,
    pub clean_report: Option<CleanReport>,
    pub should_quit: bool,

    // 菜单状态
    pub menu_index: usize,

    // 结果页状态
    pub result_scroll: usize,
    pub result_cursor: usize,
    /// 每个 category 的展开状态
    pub expanded: Vec<bool>,
    // Purge 路径
    pub purge_path: PathBuf,
    // 扫描取消标志
    pub cancel_flag: Arc<AtomicBool>,
    // 全局 spinner tick（随 throttle 节奏递增，替代各页 SystemTime 计算）
    pub tick: u64,
    /// 是否显示帮助覆盖层
    pub show_help: bool,
    /// 过滤输入模式（true 时按键编辑 `filter_query`）
    pub filter_active: bool,
    /// 当前过滤词（大小写不敏感子串匹配；空串表示不过滤）
    pub filter_query: String,
    /// TUI 统一标记集：Results 与 Analyzer 共用的"待删路径"单一来源
    pub marked: HashSet<PathBuf>,
    /// 删除确认覆盖层：Some 时弹出确认框，内含待删的 (路径, 大小) 清单
    pub confirm_delete: Option<Vec<(PathBuf, u64)>>,
}

impl App {
    pub fn new() -> Self {
        Self {
            state: AppState::Menu,
            active_command: None,
            scan_result: None,
            clean_report: None,
            should_quit: false,
            menu_index: 0,
            result_scroll: 0,
            result_cursor: 0,
            expanded: Vec::new(),
            purge_path: dirs::home_dir().unwrap_or_else(|| PathBuf::from("/")),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            tick: 0,
            show_help: false,
            filter_active: false,
            filter_query: String::new(),
            marked: HashSet::new(),
            confirm_delete: None,
        }
    }

    /// 构建扁平化的行列表，用于结果页渲染和交互
    ///
    /// 按 `SafetyLevel` 分区（Safe → Moderate → Risky），组内按 `total_size` 降序排列。
    /// 每个非空分区前插入一个 Separator 标题行。
    /// 当 `filter_query` 非空时：大小写不敏感子串匹配项名，强制展开分类只显示匹配项，
    /// 跳过无匹配的分类与分区。
    pub fn build_flat_rows(&self) -> Vec<FlatRow> {
        let result = match &self.scan_result {
            Some(r) => r,
            None => return Vec::new(),
        };

        let query = self.filter_query.to_lowercase();
        let filtering = !query.is_empty();
        let safety_order = [SafetyLevel::Safe, SafetyLevel::Moderate, SafetyLevel::Risky];

        let mut rows = Vec::new();
        for &level in &safety_order {
            let mut indices: Vec<usize> = result
                .categories
                .iter()
                .enumerate()
                .filter(|(_, cat)| Self::dominant_safety(cat) == level)
                .map(|(idx, _)| idx)
                .collect();

            if indices.is_empty() {
                continue;
            }

            indices.sort_by(|&a, &b| {
                result.categories[b]
                    .total_size
                    .cmp(&result.categories[a].total_size)
            });

            // 先构建该分区的行；过滤时跳过无匹配分类，最终无行则连 Separator 一起跳过
            let mut level_rows = Vec::new();
            for cat_idx in indices {
                let cat = &result.categories[cat_idx];
                let matching: Vec<usize> = if filtering {
                    (0..cat.items.len())
                        .filter(|&i| Self::item_name(&cat.items[i]).contains(&query))
                        .collect()
                } else {
                    Vec::new()
                };
                if filtering && matching.is_empty() {
                    continue;
                }
                // 过滤时强制展开以显示匹配项
                let expanded = if filtering {
                    true
                } else {
                    self.expanded.get(cat_idx).copied().unwrap_or(false)
                };
                level_rows.push(FlatRow::Category { cat_idx, expanded });
                if expanded {
                    if filtering {
                        for item_idx in matching {
                            level_rows.push(FlatRow::Item { cat_idx, item_idx });
                        }
                    } else {
                        for item_idx in 0..cat.items.len() {
                            level_rows.push(FlatRow::Item { cat_idx, item_idx });
                        }
                    }
                }
            }
            if level_rows.is_empty() {
                continue;
            }
            rows.push(FlatRow::Separator { level });
            rows.extend(level_rows);
        }
        rows
    }

    /// 项的显示名（文件名，小写），用于过滤匹配
    fn item_name(item: &mc_core::models::ScanItem) -> String {
        item.path
            .file_name()
            .map_or_else(
                || item.path.to_string_lossy().to_string(),
                |n| n.to_string_lossy().to_string(),
            )
            .to_lowercase()
    }

    /// 过滤词变化后把光标夹回合法范围，并避开 Separator 行
    pub fn clamp_result_cursor(&mut self) {
        let rows = self.build_flat_rows();
        if rows.is_empty() {
            self.result_cursor = 0;
            return;
        }
        if self.result_cursor >= rows.len() {
            self.result_cursor = rows.len() - 1;
        }
        self.skip_separator_backward(&rows);
        self.skip_separator_forward(&rows);
    }

    /// 计算一个 category 的主导安全等级
    fn dominant_safety(cat: &mc_core::models::CategoryGroup) -> SafetyLevel {
        if cat.items.iter().any(|i| i.safety == SafetyLevel::Risky) {
            SafetyLevel::Risky
        } else if cat.items.iter().all(|i| i.safety == SafetyLevel::Safe) {
            SafetyLevel::Safe
        } else {
            SafetyLevel::Moderate
        }
    }

    /// 初始化结果页状态
    pub fn init_results(&mut self) {
        let Some(result) = self.scan_result.as_ref() else {
            return;
        };
        let cat_count = result.categories.len();
        // 默认预选安全项（沿用旧的 selected=Safe 默认，落到统一标记集）
        let safe_paths: HashSet<PathBuf> = result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.safety == SafetyLevel::Safe)
            .map(|i| i.path.clone())
            .collect();

        self.expanded = vec![false; cat_count];
        self.result_cursor = 0;
        self.result_scroll = 0;
        self.marked = safe_paths;
        // 跳过开头的 Separator
        let rows = self.build_flat_rows();
        self.skip_separator_forward(&rows);
    }

    /// 光标向下移动，跳过 Separator 行
    pub fn move_cursor_down(&mut self) {
        let rows = self.build_flat_rows();
        let row_count = rows.len();
        if row_count > 0 && self.result_cursor < row_count - 1 {
            self.result_cursor += 1;
            self.skip_separator_forward(&rows);
        }
    }

    /// 光标向上移动，跳过 Separator 行
    pub fn move_cursor_up(&mut self) {
        if self.result_cursor > 0 {
            self.result_cursor -= 1;
            let rows = self.build_flat_rows();
            self.skip_separator_backward(&rows);
        }
    }

    fn skip_separator_forward(&mut self, rows: &[FlatRow]) {
        while self.result_cursor < rows.len()
            && matches!(rows[self.result_cursor], FlatRow::Separator { .. })
        {
            if self.result_cursor < rows.len() - 1 {
                self.result_cursor += 1;
            } else {
                break;
            }
        }
    }

    fn skip_separator_backward(&mut self, rows: &[FlatRow]) {
        while self.result_cursor > 0
            && matches!(rows[self.result_cursor], FlatRow::Separator { .. })
        {
            self.result_cursor -= 1;
        }
    }

    /// 向下翻页 `page` 行，落点跳过 Separator
    pub fn move_cursor_page_down(&mut self, page: usize) {
        let rows = self.build_flat_rows();
        let row_count = rows.len();
        if row_count == 0 {
            return;
        }
        self.result_cursor = (self.result_cursor + page).min(row_count - 1);
        self.skip_separator_forward(&rows);
    }

    /// 向上翻页 `page` 行，落点跳过 Separator
    pub fn move_cursor_page_up(&mut self, page: usize) {
        let rows = self.build_flat_rows();
        if rows.is_empty() {
            return;
        }
        self.result_cursor = self.result_cursor.saturating_sub(page);
        // 先向上跳过中间 Separator；若回退到起始 Separator(索引 0)则再向前跳
        self.skip_separator_backward(&rows);
        self.skip_separator_forward(&rows);
    }

    /// 跳到首行（跳过起始 Separator）
    pub fn move_cursor_top(&mut self) {
        let rows = self.build_flat_rows();
        self.result_cursor = 0;
        self.skip_separator_forward(&rows);
    }

    /// 跳到末行（末行必为 Category/Item，无需向前跳）
    pub fn move_cursor_bottom(&mut self) {
        let rows = self.build_flat_rows();
        if rows.is_empty() {
            return;
        }
        self.result_cursor = rows.len() - 1;
        self.skip_separator_backward(&rows);
    }

    /// 全选安全级别的项目（加入统一标记集）
    pub fn select_all_safe(&mut self) {
        if let Some(result) = &self.scan_result {
            let paths: Vec<PathBuf> = result
                .categories
                .iter()
                .flat_map(|c| c.items.iter())
                .filter(|i| i.safety == SafetyLevel::Safe)
                .map(|i| i.path.clone())
                .collect();
            self.marked.extend(paths);
        }
    }

    /// 切换某行的选中状态（操作统一标记集 `marked`）
    pub fn toggle_selection(&mut self, row: &FlatRow) {
        match row {
            FlatRow::Separator { .. } => {}
            FlatRow::Category { cat_idx, .. } => {
                // 过滤激活时，切换分类头只作用于当前可见（匹配过滤词）的项，
                // 不波及被过滤隐藏的项——与用户在屏上所见一致。
                let query = self.filter_query.to_lowercase();
                let filtering = !query.is_empty();
                let paths: Vec<PathBuf> = self
                    .scan_result
                    .as_ref()
                    .and_then(|r| r.categories.get(*cat_idx))
                    .map(|c| {
                        c.items
                            .iter()
                            .filter(|i| !filtering || Self::item_name(i).contains(&query))
                            .map(|i| i.path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                if paths.is_empty() {
                    return;
                }
                let all_marked = paths.iter().all(|p| self.marked.contains(p));
                for p in paths {
                    if all_marked {
                        self.marked.remove(&p);
                    } else {
                        self.marked.insert(p);
                    }
                }
            }
            FlatRow::Item { cat_idx, item_idx } => {
                let path = self
                    .scan_result
                    .as_ref()
                    .and_then(|r| r.categories.get(*cat_idx))
                    .and_then(|c| c.items.get(*item_idx))
                    .map(|i| i.path.clone());
                if let Some(p) = path {
                    if !self.marked.remove(&p) {
                        self.marked.insert(p);
                    }
                }
            }
        }
    }

    /// 切换展开/折叠
    pub fn toggle_expand(&mut self, cat_idx: usize) {
        if let Some(exp) = self.expanded.get_mut(cat_idx) {
            *exp = !*exp;
        }
    }

    /// 获取选中项的总数量和总大小（基于统一标记集）
    pub fn selected_summary(&self) -> (usize, u64) {
        let mut count = 0;
        let mut size = 0u64;
        if let Some(result) = &self.scan_result {
            for cat in &result.categories {
                for item in &cat.items {
                    if self.marked.contains(&item.path) {
                        count += 1;
                        size += item.size;
                    }
                }
            }
        }
        (count, size)
    }

    /// 从 Results 收集已标记项的 (路径, 大小) 清单，用于删除确认
    pub fn results_delete_list(&self) -> Vec<(PathBuf, u64)> {
        let mut list = Vec::new();
        if let Some(result) = &self.scan_result {
            for cat in &result.categories {
                for item in &cat.items {
                    if self.marked.contains(&item.path) {
                        list.push((item.path.clone(), item.size));
                    }
                }
            }
        }
        list
    }

    /// 统计"已标记但不匹配当前过滤词"的项数（过滤词为空时恒为 0）。
    ///
    /// 标记是全局的（与 dua 一致），删除作用于全部已标记项而非仅可见行。
    /// 过滤缩窄视图时，删除确认框据此显式提示：过滤视图外仍有 N 项将被一并删除，
    /// 避免"我只看到 3 行却删了 87 项"的意外（审查 F1）。
    pub fn marked_hidden_by_filter(&self) -> usize {
        if self.filter_query.is_empty() {
            return 0;
        }
        let query = self.filter_query.to_lowercase();
        let Some(result) = &self.scan_result else {
            return 0;
        };
        result
            .categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| self.marked.contains(&i.path) && !Self::item_name(i).contains(&query))
            .count()
    }

    /// 回到菜单，重置状态
    pub fn back_to_menu(&mut self) {
        self.state = AppState::Menu;
        self.active_command = None;
        self.scan_result = None;
        self.clean_report = None;
        self.expanded.clear();
        self.result_cursor = 0;
        self.result_scroll = 0;
        self.filter_active = false;
        self.filter_query.clear();
        self.marked.clear();
        self.confirm_delete = None;
    }
}

/// 扁平化的行类型，用于结果列表渲染
#[derive(Debug, Clone)]
pub enum FlatRow {
    /// 风险分区标题行（不可选中）
    Separator { level: SafetyLevel },
    /// 分类头部行
    Category { cat_idx: usize, expanded: bool },
    /// 文件项行
    Item { cat_idx: usize, item_idx: usize },
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::models::{CategoryGroup, ScanItem, ScanResult};

    /// 构造一个含单个分类（Logs/Caches/tmp 三项，均 Safe）的 App，供过滤相关测试复用。
    fn app_with_logs_caches_tmp() -> App {
        let items = vec![
            ScanItem::new(PathBuf::from("/a/Logs"), 100, SafetyLevel::Safe, "cache".into()),
            ScanItem::new(PathBuf::from("/a/Caches"), 200, SafetyLevel::Safe, "cache".into()),
            ScanItem::new(PathBuf::from("/a/tmp"), 300, SafetyLevel::Safe, "cache".into()),
        ];
        let cat = CategoryGroup::new("cache".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.expanded = vec![true];
        app
    }

    #[test]
    fn marked_hidden_by_filter_counts_only_marked_and_nonmatching() {
        let mut app = app_with_logs_caches_tmp();
        // 三项全标记
        app.marked.insert(PathBuf::from("/a/Logs"));
        app.marked.insert(PathBuf::from("/a/Caches"));
        app.marked.insert(PathBuf::from("/a/tmp"));

        // 无过滤：恒为 0
        assert_eq!(app.marked_hidden_by_filter(), 0);

        // 过滤 "log"：仅 Logs 可见，Caches/tmp 已标记但被隐藏 → 2
        app.filter_query = "log".into();
        assert_eq!(app.marked_hidden_by_filter(), 2);

        // 大小写不敏感
        app.filter_query = "LOG".into();
        assert_eq!(app.marked_hidden_by_filter(), 2);

        // 过滤词命中全部已标记项 → 0（"a" 不在名字里，但都在路径 file_name 中？不含）
        app.filter_query = "s".into(); // Logs/Caches 含 s，tmp 不含 → tmp 隐藏且已标记 → 1
        assert_eq!(app.marked_hidden_by_filter(), 1);
    }

    #[test]
    fn marked_hidden_by_filter_ignores_unmarked_and_empty_scan() {
        let mut app = app_with_logs_caches_tmp();
        // 仅标记 Logs（可见），Caches/tmp 未标记 → 隐藏项里没有已标记的 → 0
        app.marked.insert(PathBuf::from("/a/Logs"));
        app.filter_query = "log".into();
        assert_eq!(app.marked_hidden_by_filter(), 0);

        // scan_result 为 None → 0
        let mut empty = App::new();
        empty.filter_query = "log".into();
        empty.marked.insert(PathBuf::from("/a/Logs"));
        assert_eq!(empty.marked_hidden_by_filter(), 0);
    }

    #[test]
    fn toggle_category_filtered_affects_only_visible_items() {
        let mut app = app_with_logs_caches_tmp();
        app.filter_query = "log".into(); // 仅 Logs 匹配可见
        let row = FlatRow::Category { cat_idx: 0, expanded: true };

        // 切换分类头：仅标记可见的 Logs，不波及隐藏的 Caches/tmp
        app.toggle_selection(&row);
        assert!(app.marked.contains(&PathBuf::from("/a/Logs")));
        assert!(!app.marked.contains(&PathBuf::from("/a/Caches")));
        assert!(!app.marked.contains(&PathBuf::from("/a/tmp")));

        // 再次切换：Logs 取消
        app.toggle_selection(&row);
        assert!(!app.marked.contains(&PathBuf::from("/a/Logs")));
    }

    #[test]
    fn toggle_category_unfiltered_affects_all_items() {
        let mut app = app_with_logs_caches_tmp();
        let row = FlatRow::Category { cat_idx: 0, expanded: false };
        app.toggle_selection(&row);
        assert_eq!(app.marked.len(), 3);
        app.toggle_selection(&row);
        assert_eq!(app.marked.len(), 0);
    }
}
