use crate::models::SafetyLevel;
use std::path::PathBuf;

pub enum ProgressEvent {
    Scanning { path: PathBuf },
    Found { category: String, path: PathBuf, size: u64, safety: SafetyLevel },
    CategoryDone { category: String, total_size: u64, count: usize },
    Complete,
    Error(String),
    CleaningFile { path: PathBuf },
    CleaningDone { freed: u64, count: usize },
}

pub trait ProgressReporter: Send + Sync {
    fn on_event(&self, event: ProgressEvent);
}

pub struct NoopReporter;

impl ProgressReporter for NoopReporter {
    fn on_event(&self, _event: ProgressEvent) {}
}
