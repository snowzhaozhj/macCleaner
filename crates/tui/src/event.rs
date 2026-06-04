use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use crossterm::event::{self, Event, KeyEvent};
use mc_core::progress::ProgressEvent;
use std::thread;
use std::time::Duration;

pub struct EventHandler {
    pub key_rx: Receiver<KeyEvent>,
    pub progress_rx: Receiver<ProgressEvent>,
    progress_tx: Sender<ProgressEvent>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (key_tx, key_rx) = unbounded();
        let (progress_tx, progress_rx) = bounded::<ProgressEvent>(100);

        thread::spawn(move || {
            loop {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key_tx.send(key).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Self {
            key_rx,
            progress_rx,
            progress_tx,
        }
    }

    pub fn progress_sender(&self) -> Sender<ProgressEvent> {
        self.progress_tx.clone()
    }
}
