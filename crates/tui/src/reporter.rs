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
        let _ = self.tx.send(event);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}
