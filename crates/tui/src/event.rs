use crossbeam_channel::{Receiver, Sender, unbounded};
use crossterm::event::{self, Event, KeyEvent};
use mc_core::progress::ProgressEvent;
use std::thread;
use std::time::Duration;

/// TUI 事件类型
pub enum AppEvent {
    /// 键盘事件
    Key(KeyEvent),
    /// 引擎进度事件
    Progress(ProgressEvent),
    /// 定时 tick（用于刷新 UI）
    Tick,
}

/// 事件处理器：在后台线程中轮询 crossterm 键盘事件和引擎进度事件
pub struct EventHandler {
    /// 接收端：主线程从这里读取事件
    rx: Receiver<AppEvent>,
    /// 进度事件发送端：传给 ProgressReporter 使用
    progress_tx: Sender<ProgressEvent>,
}

impl EventHandler {
    /// 创建事件处理器，启动后台键盘轮询线程
    pub fn new() -> Self {
        let (tx, rx) = unbounded();
        let (progress_tx, progress_rx) = unbounded::<ProgressEvent>();

        let tx_key = tx.clone();
        let tx_tick = tx.clone();
        let tx_progress = tx;

        // 键盘事件轮询线程
        thread::spawn(move || {
            loop {
                // 每 50ms 轮询一次键盘事件
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if tx_key.send(AppEvent::Key(key)).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        // Tick 线程（每 200ms 一次，用于刷新 spinner 等动画）
        thread::spawn(move || {
            loop {
                thread::sleep(Duration::from_millis(200));
                if tx_tick.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        // 进度事件转发线程
        thread::spawn(move || {
            while let Ok(evt) = progress_rx.recv() {
                if tx_progress.send(AppEvent::Progress(evt)).is_err() {
                    break;
                }
            }
        });

        Self { rx, progress_tx }
    }

    /// 阻塞等待下一个事件
    pub fn next(&self) -> Result<AppEvent, crossbeam_channel::RecvError> {
        self.rx.recv()
    }

    /// 获取进度事件发送端的克隆（传给 ProgressReporter）
    pub fn progress_sender(&self) -> Sender<ProgressEvent> {
        self.progress_tx.clone()
    }
}
