use mc_core::models::{CleanReport, DirNode, ScanResult};
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
        marked_for_delete: Vec<PathBuf>,
        /// 每层的 cursor 位置缓存（用于 Backspace 恢复）
        cursor_stack: Vec<usize>,
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
    // Analyzer 渐进式预览（扫描中的快照）
    pub analyze_preview: Option<DirNode>,
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
            analyze_preview: None,
        }
    }

    /// 构建扁平化的行列表，用于结果页渲染和交互
    pub fn build_flat_rows(&self) -> Vec<FlatRow> {
        let result = match &self.scan_result {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut rows = Vec::new();
        for (cat_idx, cat) in result.categories.iter().enumerate() {
            let expanded = self.expanded.get(cat_idx).copied().unwrap_or(false);
            rows.push(FlatRow::Category {
                cat_idx,
                expanded,
            });
            if expanded {
                for item_idx in 0..cat.items.len() {
                    rows.push(FlatRow::Item { cat_idx, item_idx });
                }
            }
        }
        rows
    }

    /// 初始化结果页状态
    pub fn init_results(&mut self) {
        if let Some(ref result) = self.scan_result {
            self.expanded = vec![false; result.categories.len()];
            self.result_cursor = 0;
            self.result_scroll = 0;
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
            FlatRow::Category { cat_idx, .. } => {
                if let Some(ref mut result) = self.scan_result {
                    if let Some(cat) = result.categories.get_mut(*cat_idx) {
                        // 如果全部选中则取消全部，否则全部选中
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
        self.analyze_preview = None;
        self.expanded.clear();
        self.result_cursor = 0;
        self.result_scroll = 0;
    }
}

/// 扁平化的行类型，用于结果列表渲染
#[derive(Debug, Clone)]
pub enum FlatRow {
    /// 分类头部行
    Category { cat_idx: usize, expanded: bool },
    /// 文件项行
    Item { cat_idx: usize, item_idx: usize },
}
