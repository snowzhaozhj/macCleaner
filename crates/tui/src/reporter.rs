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

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::bounded;

    #[test]
    fn forwards_events_when_not_cancelled() {
        let (tx, rx) = bounded(4);
        let flag = Arc::new(AtomicBool::new(false));
        let reporter = TuiReporter::new(tx, flag);
        reporter.on_event(ProgressEvent::Complete);
        assert!(rx.try_recv().is_ok(), "未取消时事件应正常转发");
        assert!(!reporter.is_cancelled());
    }

    #[test]
    fn drops_events_when_cancelled() {
        // 防污染核心：取消后 on_event 直接丢弃，不再写入 channel，
        // 避免残留事件在返回菜单/下一次扫描时污染 scan_result。
        let (tx, rx) = bounded(4);
        let flag = Arc::new(AtomicBool::new(false));
        let reporter = TuiReporter::new(tx, flag.clone());
        flag.store(true, Ordering::Relaxed);
        reporter.on_event(ProgressEvent::Complete);
        assert!(rx.try_recv().is_err(), "取消后事件应被丢弃，channel 保持为空");
        assert!(reporter.is_cancelled());
    }
}
