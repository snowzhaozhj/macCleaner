use crate::models::SafetyLevel;
use std::path::PathBuf;

pub enum ProgressEvent {
    Scanning { path: PathBuf },
    Found {
        category: String,
        path: PathBuf,
        size: u64,
        safety: SafetyLevel,
        impact: String,
        recovery: String,
        preselect: bool,
    },
    CategoryDone { category: String, total_size: u64, count: usize },
    RuleProgress { current: usize, total: usize, name: String },
    /// 因权限（`PermissionDenied`）读不到某目录/条目而**跳过**它（#23）。
    /// 与静默 `warn!` 吞错的区别：这是结构化事件，UI 可单列「跳过（需授权）」区并引导授权。
    /// **只承载权限类跳过**——其它 IO 错误不走这个变体，避免把"文件系统坏了"误报成"缺授权"。
    SkippedNoPermission { path: PathBuf },
    Complete,
    Error(String),
    CleaningFile { path: PathBuf },
    /// 清理完成。`deleted_paths` 仅含**成功**移除的路径（供分析器精确剪树，
    /// 失败项须保留在视图中，避免界面与磁盘发散）。
    CleaningDone {
        freed: u64,
        count: usize,
        deleted_paths: Vec<PathBuf>,
    },
}

pub trait ProgressReporter: Send + Sync {
    fn on_event(&self, event: ProgressEvent);
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn on_event(&self, _event: ProgressEvent) {}
}

/// 磁盘分析增量遍历事件，通过独立 channel 传输，不经过 `ProgressReporter`。
pub enum AnalyzeEvent {
    /// 发现一个文件或目录 entry（每个 entry 一条）。
    /// 无 `depth` 字段：树构建改为**路径键控插入**（见 `mc_tui` 的 `IncrementalTreeBuilder`），
    /// 父子关系由路径推导，与交付顺序解耦——不再依赖遍历方（jwalk/park）给出 DFS 深度。
    Entry {
        name: String,
        path: PathBuf,
        size: u64,           // 文件的字节大小；目录为 0
        is_file: bool,
    },
    /// 进度统计快照（每 500 个 entry 发送一次）
    Progress {
        file_count: u64,
        total_size: u64,
    },
    /// 遍历完成
    Finished,
}
