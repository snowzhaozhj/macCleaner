use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use crossterm::event::{self, Event, KeyEvent, MouseEvent};
use mc_core::progress::ProgressEvent;
use std::thread;
use std::time::Duration;

pub struct EventHandler {
    pub key_rx: Receiver<KeyEvent>,
    pub mouse_rx: Receiver<MouseEvent>,
    pub progress_rx: Receiver<ProgressEvent>,
    progress_tx: Sender<ProgressEvent>,
}

impl EventHandler {
    pub fn new() -> Self {
        let (key_tx, key_rx) = unbounded();
        let (mouse_tx, mouse_rx) = unbounded();
        let (progress_tx, progress_rx) = bounded::<ProgressEvent>(100);

        thread::spawn(move || {
            loop {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    match event::read() {
                        // key_tx 失败 = 主循环已退出（Receiver drop），结束读线程。
                        // 鼠标事件尽力转发；捕获未开启时终端不会产生 Mouse 事件。
                        Ok(Event::Key(key)) => {
                            if key_tx.send(key).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Mouse(m)) => {
                            let _ = mouse_tx.send(m);
                        }
                        _ => {}
                    }
                }
            }
        });

        Self {
            key_rx,
            mouse_rx,
            progress_rx,
            progress_tx,
        }
    }

    pub fn progress_sender(&self) -> Sender<ProgressEvent> {
        self.progress_tx.clone()
    }
}
