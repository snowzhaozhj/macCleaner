use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 删除风险分级。**两条判据串联**，别当成单轴：先看数据丢失，再看重建摩擦。
///
/// - `Safe`：删除零数据丢失，且下次需要时自动、透明、按需补回，用户无需任何显式动作、
///   不留下被破坏的项目（如共享/下载缓存、`__pycache__`）。默认勾选。
/// - `Moderate`：删除零数据丢失，但会清空某个项目的完整依赖/构建产物，下次构建/运行前
///   需用户主动跑一次重装或重建（如 `node_modules`、`target`、`DerivedData` 的冷编译）。默认勾选。
/// - `Risky`：删除可能丢失不可再生数据或有价值状态（如 Docker 命名卷、含 dSYM 的归档、
///   装好环境的模拟器镜像）。默认不勾选，删除时额外确认。
///
/// 判据（决策树，非单轴）：
/// 1. 会丢不可再生数据/有价值状态？会 → `Risky`。这一层是真正的"数据丢失"轴。
/// 2. 不丢的里面，重建是否需要用户主动发起、并有明显耗时/打断？是 → `Moderate`，否 → `Safe`。
///    这一层用的就是"重建摩擦/代价"——`Safe` 与 `Moderate` 都是零数据丢失，把它俩分开的
///    正是这条轴。每项的具体代价再由 impact/recovery 证据文案细化。
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
    /// 移入废纸篓后的实际落点（`~/.Trash/<name>`）。仅 `DeleteMode::Trash` 且捕获成功时为 `Some`；
    /// 永久删除、捕获失败（读不到 `~/.Trash`、差集歧义、非 home 卷）恒 `None`。
    /// 这是 `mc undo` 确定性放回的数据源。`#[serde(default)]` 保证旧 `CleanReport` JSON 向后兼容。
    #[serde(default)]
    pub trashed_to: Option<PathBuf>,
    /// 落点文件的 inode 号。与 `trashed_to` 同时捕获（二者必须成对，缺一即视为未捕获）。
    /// 恢复时用它校验"废纸篓里这个名字仍是我们当初删的那个文件"——macOS 清空废纸篓后会**复用名字**，
    /// 仅凭路径会把无关的同名文件误恢复到原址（审查 headline）。inode 不受 xattr/Spotlight 触碰影响，
    /// 比 ctime 更稳，避免延迟 undo 被误判为已失效。
    #[serde(default)]
    pub trashed_ino: Option<u64>,
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_defaults_to_non_risky() {
        // 预选边界：Safe/Moderate 默认勾选，Risky 不勾（CLI --yes 与 TUI 预选共用此语义）。
        assert!(ScanItem::new(PathBuf::from("/a"), 1, SafetyLevel::Safe, "c".into()).selected);
        assert!(ScanItem::new(PathBuf::from("/a"), 1, SafetyLevel::Moderate, "c".into()).selected);
        assert!(!ScanItem::new(PathBuf::from("/a"), 1, SafetyLevel::Risky, "c".into()).selected);
    }

    #[test]
    fn with_preselect_false_deselects_non_risky() {
        let item = ScanItem::new(PathBuf::from("/a"), 1, SafetyLevel::Moderate, "c".into())
            .with_preselect(false);
        assert!(!item.selected, "preselect=false 的 Moderate 项不应默认勾选");
        // Risky 即便 preselect=true 也不勾选
        let risky = ScanItem::new(PathBuf::from("/a"), 1, SafetyLevel::Risky, "c".into())
            .with_preselect(true);
        assert!(!risky.selected, "Risky 无论 preselect 都不默认勾选");
    }

    #[test]
    fn selected_items_excludes_unselected() {
        let items = vec![
            ScanItem::new(PathBuf::from("/safe"), 1, SafetyLevel::Safe, "c".into()),
            ScanItem::new(PathBuf::from("/risky"), 1, SafetyLevel::Risky, "c".into()),
            ScanItem::new(PathBuf::from("/build"), 1, SafetyLevel::Moderate, "c".into())
                .with_preselect(false),
        ];
        let result = ScanResult::from_categories(vec![CategoryGroup::new("c".into(), items)]);
        let selected: Vec<_> = result.selected_items().iter().map(|i| i.path.clone()).collect();
        assert_eq!(selected, vec![PathBuf::from("/safe")], "只应含默认勾选项");
    }
}
