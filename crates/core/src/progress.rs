use crate::models::SafetyLevel;
use std::path::PathBuf;

pub enum ProgressEvent {
    Scanning { path: PathBuf },
    Found { category: String, path: PathBuf, size: u64, safety: SafetyLevel },
    CategoryDone { category: String, total_size: u64, count: usize },
    RuleProgress { current: usize, total: usize, name: String },
    Complete,
    Error(String),
    CleaningFile { path: PathBuf },
    CleaningDone { freed: u64, count: usize },
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

/// 磁盘分析增量遍历事件，通过独立 channel 传输，不经过 ProgressReporter。
pub enum AnalyzeEvent {
    /// 发现一个文件或目录 entry（每个 entry 一条）
    Entry {
        depth: usize,        // 相对于遍历根的深度，根的直接子项 depth=1
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
