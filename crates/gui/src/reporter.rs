//! `ProgressReporter` → 前端事件出口的适配器。
//!
//! 与 `mc_tui::reporter::TuiReporter` 同构：核心只调 `on_event`，我们把事件送出去；
//! 取消置位后**丢弃事件**（反污染——取消的扫描仍会短暂 emit，丢弃可避免残留事件
//! 污染下一次命令，见 TUI 同款测试）。出口抽象成 `EventSink` 以便单测。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use mc_core::progress::{ProgressEvent, ProgressReporter};
use tauri::ipc::Channel;

/// 事件出口抽象。生产实现是 Tauri 的 `ipc::Channel`；测试用收集型假实现。
pub trait EventSink: Send + Sync {
    fn send_event(&self, event: ProgressEvent);
}

impl EventSink for Channel<ProgressEvent> {
    fn send_event(&self, event: ProgressEvent) {
        // 前端断开等发送失败无需处理：与 TUI 一样忽略发送错误。
        let _ = self.send(event);
    }
}

/// 把核心进度事件转发到 `EventSink`，并承载协作式取消标志。
pub struct TauriReporter<S: EventSink> {
    sink: S,
    cancelled: Arc<AtomicBool>,
}

impl<S: EventSink> TauriReporter<S> {
    pub fn new(sink: S, cancelled: Arc<AtomicBool>) -> Self {
        Self { sink, cancelled }
    }
}

impl<S: EventSink> ProgressReporter for TauriReporter<S> {
    fn on_event(&self, event: ProgressEvent) {
        // 取消后丢弃事件，避免残留事件污染下一次扫描（反污染契约）。
        if self.cancelled.load(Ordering::Relaxed) {
            return;
        }
        self.sink.send_event(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct CollectingSink(Mutex<Vec<ProgressEvent>>);

    impl EventSink for CollectingSink {
        fn send_event(&self, event: ProgressEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    impl EventSink for Arc<CollectingSink> {
        fn send_event(&self, event: ProgressEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn forwards_events_when_not_cancelled() {
        let sink = Arc::new(CollectingSink::default());
        let flag = Arc::new(AtomicBool::new(false));
        let reporter = TauriReporter::new(sink.clone(), flag);
        reporter.on_event(ProgressEvent::Complete);
        reporter.on_event(ProgressEvent::CleaningFile { path: "/x".into() });
        assert_eq!(sink.0.lock().unwrap().len(), 2, "未取消时事件应全部转发");
    }

    #[test]
    fn drops_events_after_cancel() {
        let sink = Arc::new(CollectingSink::default());
        let flag = Arc::new(AtomicBool::new(false));
        let reporter = TauriReporter::new(sink.clone(), flag.clone());
        reporter.on_event(ProgressEvent::Complete); // 取消前：转发
        flag.store(true, Ordering::Relaxed);
        reporter.on_event(ProgressEvent::Complete); // 取消后：丢弃
        reporter.on_event(ProgressEvent::Error("x".into()));
        assert_eq!(sink.0.lock().unwrap().len(), 1, "取消后事件应被丢弃");
    }

    #[test]
    fn is_cancelled_reflects_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let reporter = TauriReporter::new(Arc::new(CollectingSink::default()), flag.clone());
        assert!(!reporter.is_cancelled());
        flag.store(true, Ordering::Relaxed);
        assert!(reporter.is_cancelled());
    }
}
