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
    /// 正在扫描。已发现项数/总大小不在此冗余存储，直接由 `scan_result` 派生
    /// （见 `ui::scan::render_scan_header`），避免两处计数需手动保持一致。
    Scanning {
        progress_text: String,
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
    pub confirm_delete: Option<Vec<ConfirmItem>>,
    /// 含 Risky 项时的 type-to-confirm 输入缓冲（D4）。
    pub confirm_input: String,
    /// 确认框清单区的滚动偏移（可见行起点）；打开/关闭确认框时归零（KTD3）。
    pub confirm_scroll: usize,
    /// Results 发起删除时暂存的完整待删清单（含 category/size），供 Done 屏计算成功/失败
    /// 明细与分类小结（KTD6）。分析器路径走 `analyzer_return`，不用此字段。
    pub clean_request: Vec<ConfirmItem>,
    /// Done 屏的结构化清理报告；None 时 Done 退回单行 message（空扫描/错误）。
    pub done_report: Option<DoneReport>,
    /// 从磁盘分析器发起删除时暂存的树与导航状态；删除在后台线程完成后据此
    /// **剪除已删节点并原地返回分析器**，而非拆树回菜单（修复"删除后莫名退出"）。
    pub analyzer_return: Option<AnalyzerReturn>,
    /// 瞬时状态提示（如"扫描中不可标记"），渲染在底部一行，下一次按键即清除。
    pub status_message: Option<String>,
    /// 沉没成本二次确认：存在已标记项时，第一次按 q 返回菜单只置位并提示，
    /// 第二次按才真正放弃标记返回（对齐 dua 的 `pending_exit`，避免手滑丢标记）。
    pub pending_leave: bool,
    /// `AnalyzingLive` 态提交删除的暂存清单（KTD1）：live 删除须先 finalize 部分树，
    /// 故在 `confirm_accept` 暂存待删 (路径, 大小)，待 `SortDone` 进入稳定 `Analyzing` 态后
    /// 再经 `start_cleaning_from_analyzer` 执行。非空即表示"本次 finalize 是为删除而非扫描完成"。
    pub pending_analyzer_delete: Vec<(PathBuf, u64)>,
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
            confirm_input: String::new(),
            confirm_scroll: 0,
            clean_request: Vec::new(),
            done_report: None,
            analyzer_return: None,
            status_message: None,
            pending_leave: false,
            pending_analyzer_delete: Vec::new(),
        }
    }

    /// 构建扁平化的行列表，用于结果页渲染和交互
    ///
    /// 按 `SafetyLevel` 分区（Safe → Moderate → Risky），组内按分类发现(插入)顺序稳定排列
    /// （扫描前后一致，避免完成瞬间重排跳变）。每个非空分区前插入一个 Separator 标题行。
    /// 当 `filter_query` 非空时：大小写不敏感子串匹配项名，强制展开分类只显示匹配项，
    /// 跳过无匹配的分类与分区。
    pub fn build_flat_rows(&self) -> Vec<FlatRow> {
        let result = match &self.scan_result {
            Some(r) => r,
            None => return Vec::new(),
        };

        let query = self.filter_query.to_lowercase();
        let filtering = !query.is_empty();
        // 分类顺序始终按发现(插入)顺序稳定排列：扫描中不因各分类 size 增长而逐帧重排
        // (跳变)，扫描完成切到 Results 时顺序、展开态、光标全都不变——消除完成瞬间的跳变。
        let safety_order = [SafetyLevel::Safe, SafetyLevel::Moderate, SafetyLevel::Risky];
        // 每个分类的主导安全等级只算一次（此前在 3 个 safety 分区里各算一遍）。
        let dominant: Vec<SafetyLevel> =
            result.categories.iter().map(Self::dominant_safety).collect();

        let mut rows = Vec::new();
        for &level in &safety_order {
            // (0..len).filter 天然升序，即发现(插入)顺序，无需再 sort。
            let indices: Vec<usize> = (0..result.categories.len())
                .filter(|&idx| dominant[idx] == level)
                .collect();

            if indices.is_empty() {
                continue;
            }

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

    /// 当前光标所在行的详情面板内容（U5）。光标可能落在 Separator/Category/Item 任一行，
    /// 各自给出对应说明，保证任何位置都有可读内容、不 panic。
    pub fn current_detail(&self) -> DetailView {
        let rows = self.build_flat_rows();
        let (Some(row), Some(result)) = (rows.get(self.result_cursor), self.scan_result.as_ref())
        else {
            return DetailView::Empty;
        };
        match row {
            FlatRow::Separator { level } => DetailView::Level(*level),
            FlatRow::Category { cat_idx, .. } => {
                let cat = &result.categories[*cat_idx];
                DetailView::Level(Self::dominant_safety(cat))
            }
            FlatRow::Item { cat_idx, item_idx } => {
                let item = &result.categories[*cat_idx].items[*item_idx];
                DetailView::Item {
                    path: item.path.clone(),
                    safety: item.safety,
                    impact: item.impact.clone(),
                    recovery: item.recovery.clone(),
                }
            }
        }
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
        // KTD3：预选（= safety != Risky && rule.preselect）已在扫描期随 Found 首次插入时
        // 增量播种到 marked，此处**不再重播种**——否则会冲掉用户扫描期的手动勾选/取消
        // （假反馈）。仅整理展开态与光标。
        // 保留扫描期间的展开态/光标，避免扫描完成瞬间"展开态跳变、列表回弹"。
        // resize 仅防御（Found 处理已维持 expanded.len()==categories.len() 不变式）。
        self.expanded.resize(cat_count, false);
        let rows = self.build_flat_rows();
        if !rows.is_empty() && self.result_cursor >= rows.len() {
            self.result_cursor = rows.len() - 1;
        }
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

    /// 全选所有非 Risky 项（`a` 键）。手动全选会覆盖 preselect=false（如 dist/build 也被选中），
    /// 但 Risky 项不纳入——需用户逐项确认。
    pub fn select_all_safe(&mut self) {
        if let Some(result) = &self.scan_result {
            let paths: Vec<PathBuf> = result
                .categories
                .iter()
                .flat_map(|c| c.items.iter())
                .filter(|i| i.safety != SafetyLevel::Risky)
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

    /// `h`/`←` 折叠：折叠光标所在分类，或把光标从子项移回其分类头（KTD4，与 Analyze 的
    /// "返回上级"最接近的二级语义）。返回 `true` 表示发生了折叠/聚焦；`false` 表示光标处
    /// 无可折叠项（已折叠的分类头 / 分隔行 / 空列表），由调用方决定根层回退（如返回菜单）。
    pub fn collapse_or_focus_category(&mut self, rows: &[FlatRow]) -> bool {
        let cat_idx = match rows.get(self.result_cursor) {
            Some(FlatRow::Item { cat_idx, .. }) => *cat_idx,
            Some(FlatRow::Category { cat_idx, expanded: true }) => *cat_idx,
            _ => return false, // 已折叠的分类头 / 分隔行 / 越界：无可折叠
        };
        if let Some(exp) = self.expanded.get_mut(cat_idx) {
            *exp = false;
        }
        // 光标移到该分类头行（折叠后子项行消失，避免光标悬空）。
        let new_rows = self.build_flat_rows();
        if let Some(pos) = new_rows
            .iter()
            .position(|r| matches!(r, FlatRow::Category { cat_idx: ci, .. } if *ci == cat_idx))
        {
            self.result_cursor = pos;
        }
        self.clamp_result_cursor();
        true
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
    pub fn results_delete_list(&self) -> Vec<ConfirmItem> {
        let mut list = Vec::new();
        if let Some(result) = &self.scan_result {
            for cat in &result.categories {
                for item in &cat.items {
                    if self.marked.contains(&item.path) {
                        list.push(ConfirmItem {
                            path: item.path.clone(),
                            size: item.size,
                            safety: item.safety,
                            category: item.category.clone(),
                            impact: item.impact.clone(),
                            recovery: item.recovery.clone(),
                        });
                    }
                }
            }
        }
        list
    }

    /// 待删集合是否含 Risky 项（决定确认框是否升级为 type-to-confirm，D4）。
    pub fn confirm_has_risky(&self) -> bool {
        self.confirm_delete.as_ref().is_some_and(|list| {
            list.iter().any(|i| i.safety == SafetyLevel::Risky)
        })
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
        self.filter_active = false;
        self.filter_query.clear();
        self.marked.clear();
        self.confirm_delete = None;
        self.confirm_input.clear();
        self.confirm_scroll = 0;
        self.clean_request = Vec::new();
        self.done_report = None;
        self.analyzer_return = None;
        self.status_message = None;
        self.pending_leave = false;
        self.pending_analyzer_delete.clear();
    }

    /// KTD2 诚实披露：勾选父目录会连带其中**未勾选**的子项一并删除（size 归属已扣除、
    /// 物理包含无法扣除）。为确认框每个"含未勾选子项"的待删父项生成一条 ⚠ 警示文案。
    /// 仅 Results 模式（有 `scan_result`）适用；分析器 `collect_marked` 已剪除标记目录的子项。
    pub fn unmarked_child_disclosures(&self) -> Vec<String> {
        let (Some(list), Some(result)) = (&self.confirm_delete, &self.scan_result) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for p in list {
            let mut count = 0usize;
            let mut example: Option<&str> = None;
            for cat in &result.categories {
                for item in &cat.items {
                    if item.path != p.path
                        && item.path.starts_with(&p.path)
                        && !self.marked.contains(&item.path)
                    {
                        count += 1;
                        if example.is_none() {
                            example = Some(cat.name.as_str());
                        }
                    }
                }
            }
            if count > 0 {
                let parent = if p.category.is_empty() { "该项" } else { p.category.as_str() };
                let ex = example.unwrap_or("其他分类");
                out.push(format!(
                    "⚠ {parent} 包含 {count} 个未勾选的子项（如 {ex}），将一并删除"
                ));
            }
        }
        out
    }
}

/// 分析器删除的返回上下文：删除在后台线程执行期间暂存整棵树与光标位置，
/// 完成后剪除 `deleted` 中的路径、修正各层大小，再恢复到暂存的导航位置。
pub struct AnalyzerReturn {
    pub tree: Arc<DirNode>,
    pub nav_path: Vec<usize>,
    pub cursor: usize,
    pub cursor_stack: Vec<usize>,
    pub deleted: Vec<PathBuf>,
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

/// 删除确认框的单项：携带 safety/impact/recovery 以支持 Risky 强调与证据展示（U7/R9），
/// 并携带 category 以支持非 Risky 项按分类汇总（KTD3）。分析器发起的删除无规则分类，category 为空。
#[derive(Debug, Clone)]
pub struct ConfirmItem {
    pub path: PathBuf,
    pub size: u64,
    pub safety: SafetyLevel,
    pub category: String,
    pub impact: String,
    pub recovery: String,
}

/// Done 屏的结构化清理报告（KTD6）：成功/失败明细 + 分类小结，取代旧的单行文案。
/// 从"暂存的待删清单"与"CleaningDone 回报的成功路径"派生，不新增冗余存储。
#[derive(Debug, Clone)]
pub struct DoneReport {
    /// 实际释放字节（移入废纸篓的成功项之和）
    pub freed: u64,
    /// 成功项数
    pub succeeded: usize,
    /// 失败项路径（请求删除但未出现在成功清单中的项，权限/占用/SIP 等）
    pub failed_paths: Vec<PathBuf>,
    /// 成功项按分类小结：(分类名, 项数, 大小)，按发现顺序稳定
    pub categories: Vec<(String, usize, u64)>,
}

impl DoneReport {
    /// 由暂存待删清单 `request` 与成功路径 `deleted` 派生报告。
    pub fn from_request(request: &[ConfirmItem], freed: u64, deleted: &[PathBuf]) -> Self {
        let deleted_set: HashSet<&PathBuf> = deleted.iter().collect();
        let failed_paths: Vec<PathBuf> = request
            .iter()
            .filter(|i| !deleted_set.contains(&i.path))
            .map(|i| i.path.clone())
            .collect();
        // 分类小结：仅统计成功项，保持首次出现顺序。
        let mut categories: Vec<(String, usize, u64)> = Vec::new();
        for item in request.iter().filter(|i| deleted_set.contains(&i.path)) {
            let label = if item.category.is_empty() { "待删项" } else { item.category.as_str() };
            if let Some(entry) = categories.iter_mut().find(|(name, _, _)| name == label) {
                entry.1 += 1;
                entry.2 += item.size;
            } else {
                categories.push((label.to_string(), 1, item.size));
            }
        }
        Self {
            freed,
            succeeded: deleted.len(),
            failed_paths,
            categories,
        }
    }
}

/// 结果页详情面板内容（U5）：随光标位置给出可读说明。
#[derive(Debug, Clone)]
pub enum DetailView {
    /// 无扫描结果或空列表
    Empty,
    /// 光标在分区/分类行：展示该等级的 rubric 一句话
    Level(SafetyLevel),
    /// 光标在文件项：展示该项完整路径 + 影响与恢复方式（evidence 为空则显示占位）
    Item {
        path: PathBuf,
        safety: SafetyLevel,
        impact: String,
        recovery: String,
    },
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

    /// 构造混合安全级别的 App（Safe / Moderate / Risky / preselect=false 各一）。
    fn app_mixed_safety() -> App {
        let items = vec![
            ScanItem::new(PathBuf::from("/x/cache"), 10, SafetyLevel::Safe, "c".into()),
            ScanItem::new(PathBuf::from("/x/node_modules"), 20, SafetyLevel::Moderate, "c".into()),
            ScanItem::new(PathBuf::from("/x/docker"), 30, SafetyLevel::Risky, "c".into()),
            ScanItem::new(PathBuf::from("/x/build"), 40, SafetyLevel::Moderate, "c".into())
                .with_preselect(false),
        ];
        let cat = CategoryGroup::new("c".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.expanded = vec![true];
        app
    }

    #[test]
    fn results_delete_list_carries_safety_and_evidence() {
        let items = vec![
            ScanItem::new(PathBuf::from("/x/docker"), 30, SafetyLevel::Risky, "Docker".into())
                .with_evidence("卷内数据丢失".into(), "不可恢复".into()),
            ScanItem::new(PathBuf::from("/x/nm"), 20, SafetyLevel::Moderate, "Node.js".into()),
        ];
        let cat = CategoryGroup::new("c".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.marked.insert(PathBuf::from("/x/docker"));
        app.marked.insert(PathBuf::from("/x/nm"));

        let list = app.results_delete_list();
        let docker = list.iter().find(|i| i.path.ends_with("docker")).unwrap();
        assert_eq!(docker.safety, SafetyLevel::Risky);
        assert_eq!(docker.impact, "卷内数据丢失");
        assert_eq!(docker.recovery, "不可恢复");

        app.confirm_delete = Some(list);
        assert!(app.confirm_has_risky(), "含 Docker(Risky) 应触发 type-to-confirm");
    }

    #[test]
    fn confirm_has_risky_false_without_risky() {
        let mut app = App::new();
        app.confirm_delete = Some(vec![ConfirmItem {
            path: PathBuf::from("/x/nm"),
            size: 1,
            safety: SafetyLevel::Moderate,
            category: "c".into(),
            impact: String::new(),
            recovery: String::new(),
        }]);
        assert!(!app.confirm_has_risky());
    }

    #[test]
    fn current_detail_resolves_item_and_level_rows() {
        let items = vec![ScanItem::new(PathBuf::from("/x/nm"), 20, SafetyLevel::Moderate, "c".into())
            .with_evidence("依赖被清空".into(), "npm install".into())];
        let cat = CategoryGroup::new("c".into(), items);
        let mut app = App::new();
        app.scan_result = Some(ScanResult::from_categories(vec![cat]));
        app.expanded = vec![true];

        let rows = app.build_flat_rows();
        // 光标落在 Item 行 → DetailView::Item 带 impact/recovery
        let item_idx = rows
            .iter()
            .position(|r| matches!(r, FlatRow::Item { .. }))
            .expect("应有 Item 行");
        app.result_cursor = item_idx;
        match app.current_detail() {
            DetailView::Item { impact, recovery, safety, .. } => {
                assert_eq!(safety, SafetyLevel::Moderate);
                assert_eq!(impact, "依赖被清空");
                assert_eq!(recovery, "npm install");
            }
            other => panic!("Item 行应返回 DetailView::Item，实际 {other:?}"),
        }

        // 光标落在分区/分类行 → DetailView::Level，不 panic
        let sep_idx = rows
            .iter()
            .position(|r| matches!(r, FlatRow::Separator { .. }))
            .expect("应有 Separator 行");
        app.result_cursor = sep_idx;
        assert!(matches!(app.current_detail(), DetailView::Level(_)));

        // 无扫描结果 → Empty
        let empty = App::new();
        assert!(matches!(empty.current_detail(), DetailView::Empty));
    }

    #[test]
    fn init_results_does_not_clobber_scan_time_marks() {
        // KTD3：预选改在扫描期随 Found 播种；init_results 不再重播种，
        // 保留用户扫描期的手动勾选/取消，完成瞬间不冲掉（消除假反馈）。
        let mut app = app_mixed_safety();
        // 模拟扫描期标记状态：保留一个预选项、手动加入一个 preselect=false 项、
        // 手动取消一个本会预选的 Moderate 项（未加入 marked）。
        app.marked.insert(PathBuf::from("/x/cache"));
        app.marked.insert(PathBuf::from("/x/build"));
        app.init_results();
        assert!(app.marked.contains(&PathBuf::from("/x/cache")), "手动保留项应在");
        assert!(app.marked.contains(&PathBuf::from("/x/build")), "手动加入项应保留（preselect=false 也不被剔除）");
        assert!(
            !app.marked.contains(&PathBuf::from("/x/node_modules")),
            "被取消的预选不应被重播回"
        );
        assert!(!app.marked.contains(&PathBuf::from("/x/docker")), "Risky 不应出现");
    }

    #[test]
    fn select_all_safe_includes_moderate_and_preselect_false_but_not_risky() {
        // `a` 键手动全选：覆盖 preselect=false（build 也选中），但仍排除 Risky。
        let mut app = app_mixed_safety();
        app.select_all_safe();
        assert!(app.marked.contains(&PathBuf::from("/x/cache")));
        assert!(app.marked.contains(&PathBuf::from("/x/node_modules")));
        assert!(app.marked.contains(&PathBuf::from("/x/build")), "手动全选应含 build");
        assert!(!app.marked.contains(&PathBuf::from("/x/docker")), "Risky 仍不选");
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
    fn collapse_or_focus_from_item_collapses_and_focuses_head() {
        // U6/KTD4：光标在子项按 h → 折叠父分类并把光标移回分类头。
        let mut app = app_with_logs_caches_tmp(); // expanded=[true]
        let rows = app.build_flat_rows();
        let item_pos = rows.iter().position(|r| matches!(r, FlatRow::Item { .. })).unwrap();
        app.result_cursor = item_pos;
        assert!(app.collapse_or_focus_category(&rows), "应发生折叠");
        assert!(!app.expanded[0], "父分类应折叠");
        let new_rows = app.build_flat_rows();
        assert!(
            matches!(new_rows[app.result_cursor], FlatRow::Category { .. }),
            "光标应落在分类头"
        );
    }

    #[test]
    fn collapse_or_focus_on_collapsed_head_is_noop() {
        // 已折叠的分类头无可折叠 → 返回 false，交调用方决定根层回退。
        let mut app = app_with_logs_caches_tmp();
        app.expanded = vec![false];
        let rows = app.build_flat_rows();
        let head_pos = rows.iter().position(|r| matches!(r, FlatRow::Category { .. })).unwrap();
        app.result_cursor = head_pos;
        assert!(!app.collapse_or_focus_category(&rows), "已折叠头应返回 false");
    }

    #[test]
    fn collapse_or_focus_on_expanded_head_collapses() {
        let mut app = app_with_logs_caches_tmp(); // expanded=[true]
        let rows = app.build_flat_rows();
        let head_pos = rows.iter().position(|r| matches!(r, FlatRow::Category { .. })).unwrap();
        app.result_cursor = head_pos;
        assert!(app.collapse_or_focus_category(&rows));
        assert!(!app.expanded[0], "展开的分类头按 h 应折叠");
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
