use crate::models::SafetyLevel;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 派生 `Serialize`/`Deserialize`：GUI 后端（Tauri）需把事件过 `ipc::Channel` 序列化推给前端。
/// 内含的 `PathBuf`/`SafetyLevel` 均已可序列化，此处为加性改动，不影响 CLI/TUI 的 channel 传输。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
/// 同样派生 `Serialize`/`Deserialize` 供 GUI 后端过 `ipc::Channel` 推送。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalyzeEvent {
    /// 发现一个文件或目录 entry（每个 entry 一条）。
    /// 无 `depth` 字段：树构建改为**路径键控插入**（见 `IncrementalTreeBuilder`），
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

#[cfg(test)]
mod tests {
    use super::*;

    /// GUI 后端会把 `ProgressEvent` 过 `ipc::Channel` 序列化推给前端——
    /// 保证含 `SafetyLevel::Risky` + 非空 impact/recovery 的 `Found` 变体 round-trip 保真。
    #[test]
    fn progress_event_found_serde_round_trip() {
        let evt = ProgressEvent::Found {
            category: "浏览器缓存".into(),
            path: PathBuf::from("/Users/x/Library/Caches/foo"),
            size: 4096,
            safety: SafetyLevel::Risky,
            impact: "可能含未同步状态".into(),
            recovery: "重新登录即可".into(),
            preselect: false,
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: ProgressEvent = serde_json::from_str(&json).unwrap();
        match back {
            ProgressEvent::Found { category, size, safety, impact, recovery, preselect, .. } => {
                assert_eq!(category, "浏览器缓存");
                assert_eq!(size, 4096);
                assert_eq!(safety, SafetyLevel::Risky);
                assert_eq!(impact, "可能含未同步状态");
                assert_eq!(recovery, "重新登录即可");
                assert!(!preselect);
            }
            other => panic!("round-trip 变体错误: {other:?}"),
        }
    }

    /// `SkippedNoPermission` 是 FDA UX 的结构化信号，序列化字段须稳定。
    #[test]
    fn progress_event_skipped_serde_round_trip() {
        let evt = ProgressEvent::SkippedNoPermission { path: PathBuf::from("/Users/x/Library/Mail") };
        let json = serde_json::to_string(&evt).unwrap();
        let back: ProgressEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ProgressEvent::SkippedNoPermission { path } if path == std::path::Path::new("/Users/x/Library/Mail")));
    }

    /// `AnalyzeEvent` 三变体（含 `is_file` true/false）序列化保真。
    #[test]
    fn analyze_event_serde_round_trip() {
        for evt in [
            AnalyzeEvent::Entry { name: "node_modules".into(), path: PathBuf::from("/p/node_modules"), size: 0, is_file: false },
            AnalyzeEvent::Entry { name: "big.bin".into(), path: PathBuf::from("/p/big.bin"), size: 1 << 30, is_file: true },
            AnalyzeEvent::Progress { file_count: 500, total_size: 12345 },
            AnalyzeEvent::Finished,
        ] {
            let json = serde_json::to_string(&evt).unwrap();
            let _back: AnalyzeEvent = serde_json::from_str(&json).unwrap();
        }
    }
}
