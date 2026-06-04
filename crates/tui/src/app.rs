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
    /// 确认清理
    Confirming,
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
        /// 标记为删除的路径
        marked_for_delete: HashSet<PathBuf>,
        /// 每层的 cursor 位置缓存（用于 Backspace 恢复）
        cursor_stack: Vec<usize>,
        /// 是否为部分扫描结果（用户取消扫描后保留的不完整树）
        partial: bool,
    },
    /// 磁盘分析进行中（增量构建 + 可导航）
    AnalyzingLive {
        tree_root: DirNode,           // owned，非 Arc，正在增量构建
        nav_path: Vec<usize>,
        cursor: usize,
        marked_for_delete: HashSet<PathBuf>,
        cursor_stack: Vec<usize>,
        file_count: u64,              // 已发现的文件总数
        total_size: u64,              // 已累计的字节总量
        user_navigated: bool,         // 用户是否已手动导航（true 后 cursor 不再自动跟随）
    },
    /// 正在排序（finalize 在后台线程执行）
    Sorting {
        marked_for_delete: HashSet<PathBuf>,
        cursor_stack: Vec<usize>,
        partial: bool,
    },
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
        }
    }

    /// 构建扁平化的行列表，用于结果页渲染和交互
    ///
    /// 按 SafetyLevel 分区（Safe → Moderate → Risky），组内按 total_size 降序排列。
    /// 每个非空分区前插入一个 Separator 标题行。
    pub fn build_flat_rows(&self) -> Vec<FlatRow> {
        let result = match &self.scan_result {
            Some(r) => r,
            None => return Vec::new(),
        };

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

            rows.push(FlatRow::Separator { level });
            for cat_idx in indices {
                let expanded = self.expanded.get(cat_idx).copied().unwrap_or(false);
                rows.push(FlatRow::Category { cat_idx, expanded });
                if expanded {
                    for item_idx in 0..result.categories[cat_idx].items.len() {
                        rows.push(FlatRow::Item { cat_idx, item_idx });
                    }
                }
            }
        }
        rows
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
        if let Some(ref result) = self.scan_result {
            self.expanded = vec![false; result.categories.len()];
            self.result_cursor = 0;
            self.result_scroll = 0;
            // 跳过开头的 Separator
            let rows = self.build_flat_rows();
            self.skip_separator_forward(&rows);
        }
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

    /// 全选安全级别的项目
    pub fn select_all_safe(&mut self) {
        if let Some(ref mut result) = self.scan_result {
            for cat in &mut result.categories {
                for item in &mut cat.items {
                    if item.safety == mc_core::models::SafetyLevel::Safe {
                        item.selected = true;
                    }
                }
            }
        }
    }

    /// 切换某行的选中状态
    pub fn toggle_selection(&mut self, row: &FlatRow) {
        match row {
            FlatRow::Separator { .. } => {}
            FlatRow::Category { cat_idx, .. } => {
                if let Some(ref mut result) = self.scan_result {
                    if let Some(cat) = result.categories.get_mut(*cat_idx) {
                        let all_selected = cat.items.iter().all(|i| i.selected);
                        for item in &mut cat.items {
                            item.selected = !all_selected;
                        }
                    }
                }
            }
            FlatRow::Item { cat_idx, item_idx } => {
                if let Some(ref mut result) = self.scan_result {
                    if let Some(cat) = result.categories.get_mut(*cat_idx) {
                        if let Some(item) = cat.items.get_mut(*item_idx) {
                            item.selected = !item.selected;
                        }
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

    /// 获取选中项的总数量和总大小
    pub fn selected_summary(&self) -> (usize, u64) {
        match &self.scan_result {
            Some(result) => {
                let items = result.selected_items();
                let size: u64 = items.iter().map(|i| i.size).sum();
                (items.len(), size)
            }
            None => (0, 0),
        }
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
