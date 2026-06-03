use crate::models::{DirNode, SafetyLevel};
use std::path::PathBuf;

pub enum ProgressEvent {
    Scanning { path: PathBuf },
    Found { category: String, path: PathBuf, size: u64, safety: SafetyLevel },
    CategoryDone { category: String, total_size: u64, count: usize },
    RuleProgress { current: usize, total: usize, name: String },
    AnalyzeSnapshot { tree: DirNode },
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
