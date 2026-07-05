use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 删除风险分级（单轴判据："删了会不会丢不可再生的东西"）。
///
/// - `Safe`：删除零数据丢失，且下次需要时自动、透明、按需补回，用户无需任何显式动作、
///   不留下被破坏的项目（如共享/下载缓存、`__pycache__`）。默认勾选。
/// - `Moderate`：删除零数据丢失，但会清空某个项目的完整依赖/构建产物，下次构建/运行前
///   需用户显式跑一次重装或重建命令（如 `node_modules`、`target`）。默认勾选。
/// - `Risky`：删除可能丢失不可再生数据或有价值状态（如 Docker 命名卷、含 dSYM 的归档、
///   装好环境的模拟器镜像）。默认不勾选，删除时额外确认。
///
/// 判据口诀：删了会不会丢不可再生的东西？会 → Risky；不会但要用户手动重建一个项目 →
/// Moderate；不会且自动按需补回 → Safe。"重建代价"不进本轴，改由每项的 impact/recovery
/// 证据文案承载。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyLevel {
    Safe,
    Moderate,
    Risky,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeleteMode {
    Trash,
    Permanent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanItem {
    pub path: PathBuf,
    pub size: u64,
    pub safety: SafetyLevel,
    pub category: String,
    pub selected: bool,
    /// 删除后果一句话（"删了会怎样"），最坏情况优先。非规则来源可为空串。
    pub impact: String,
    /// 恢复方式一句话（"如何恢复"），不可恢复的写明。非规则来源可为空串。
    pub recovery: String,
}

impl ScanItem {
    /// 创建扫描项。默认预选边界为"非 Risky 即勾选"（等价 `preselect = true`）；
    /// 需要覆盖预选的场景（如 dist/build）用 [`ScanItem::with_preselect`]。
    pub fn new(path: PathBuf, size: u64, safety: SafetyLevel, category: String) -> Self {
        Self {
            path,
            size,
            safety,
            selected: safety != SafetyLevel::Risky,
            category,
            impact: String::new(),
            recovery: String::new(),
        }
    }

    /// 附加证据文案（链式）。不改变预选状态。
    #[must_use]
    pub fn with_evidence(mut self, impact: String, recovery: String) -> Self {
        self.impact = impact;
        self.recovery = recovery;
        self
    }

    /// 应用规则的 `preselect` 覆盖（链式）：`selected = safety != Risky && preselect`。
    #[must_use]
    pub fn with_preselect(mut self, preselect: bool) -> Self {
        self.selected = self.safety != SafetyLevel::Risky && preselect;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryGroup {
    pub name: String,
    pub items: Vec<ScanItem>,
    pub total_size: u64,
    pub file_count: usize,
}

impl CategoryGroup {
    pub fn new(name: String, items: Vec<ScanItem>) -> Self {
        let total_size = items.iter().map(|i| i.size).sum();
        let file_count = items.len();
        Self {
            name,
            items,
            total_size,
            file_count,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub categories: Vec<CategoryGroup>,
    pub total_size: u64,
    pub file_count: usize,
}

impl ScanResult {
    pub fn from_categories(categories: Vec<CategoryGroup>) -> Self {
        let total_size = categories.iter().map(|c| c.total_size).sum();
        let file_count = categories.iter().map(|c| c.file_count).sum();
        Self {
            categories,
            total_size,
            file_count,
        }
    }

    pub fn selected_items(&self) -> Vec<&ScanItem> {
        self.categories
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| i.selected)
            .collect()
    }

}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanedItem {
    pub path: PathBuf,
    pub size: u64,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CleanReport {
    pub cleaned: Vec<CleanedItem>,
    pub total_freed: u64,
    pub success_count: usize,
    pub failure_count: usize,
}

impl CleanReport {
    pub fn add(&mut self, item: CleanedItem) {
        if item.success {
            self.total_freed += item.size;
            self.success_count += 1;
        } else {
            self.failure_count += 1;
        }
        self.cleaned.push(item);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: PathBuf,
    pub size: u64,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirNode {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub children: Vec<DirNode>,
    pub is_file: bool,
}

impl DirNode {
    pub fn new_dir(path: PathBuf, name: String) -> Self {
        Self {
            path,
            name,
            size: 0,
            children: Vec::new(),
            is_file: false,
        }
    }

    pub fn new_file(path: PathBuf, name: String, size: u64) -> Self {
        Self {
            path,
            name,
            size,
            children: Vec::new(),
            is_file: true,
        }
    }
}

