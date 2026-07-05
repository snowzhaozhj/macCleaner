use crossbeam_channel::Sender;
use mc_core::progress::{ProgressEvent, ProgressReporter};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct TuiReporter {
    tx: Sender<ProgressEvent>,
    cancelled: Arc<AtomicBool>,
}

impl TuiReporter {
    pub fn new(tx: Sender<ProgressEvent>, cancelled: Arc<AtomicBool>) -> Self {
        Self { tx, cancelled }
    }
}

impl ProgressReporter for TuiReporter {
    fn on_event(&self, event: ProgressEvent) {
        // 协作式取消有延迟：扫描线程被取消后仍会跑一小段并继续 emit。已取消则直接丢弃，
        // 避免残留事件在返回菜单后重建 scan_result、或串入下一次扫描（污染新命令的列表）。
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        let _ = self.tx.send(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
