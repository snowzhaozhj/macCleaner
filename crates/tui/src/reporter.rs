use crossbeam_channel::Sender;
use mc_core::progress::{ProgressEvent, ProgressReporter};

/// TUI 进度报告器：将引擎事件通过 channel 发送给 UI 线程
pub struct TuiReporter {
    tx: Sender<ProgressEvent>,
}

impl TuiReporter {
    pub fn new(tx: Sender<ProgressEvent>) -> Self {
        Self { tx }
    }
}

impl ProgressReporter for TuiReporter {
    fn on_event(&self, event: ProgressEvent) {
        // 发送失败时静默忽略（UI 可能已退出）
        let _ = self.tx.send(event);
    }
}
