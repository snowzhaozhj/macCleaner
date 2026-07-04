use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// 渲染频率门控器。
/// 后台线程每 `duration` 将 trigger 设为 true；
/// 主线程通过 `can_update()` 原子交换读取并重置。
/// 当 Throttle 被 drop 后，Arc `释放，Weak::upgrade` 返回 None，后台线程自动退出。
pub(crate) struct Throttle {
    trigger: Arc<AtomicBool>,
}

impl Throttle {
    /// 创建 Throttle，后台线程每 `duration` 设置 trigger 为 true。
    /// trigger 初始值为 true，确保首帧立即渲染。
    /// Drop 后 `Weak::upgrade` 失败，后台线程自动退出（最多延迟一个 duration）。
    pub(crate) fn new(duration: Duration) -> Self {
        let instance = Self {
            trigger: Arc::new(AtomicBool::new(true)), // 首帧立即放行
        };

        let weak = Arc::downgrade(&instance.trigger);
        std::thread::Builder::new()
            .name("mc-render-throttle".into())
            .spawn(move || {
                while let Some(t) = weak.upgrade() {
                    t.store(true, Ordering::Relaxed);
                    drop(t); // 必须在 sleep 前 drop，否则 Weak::upgrade 永远成功
                    std::thread::sleep(duration);
                }
            })
            .expect("spawn throttle thread");

        instance
    }

    /// 原子交换：返回 true 表示允许渲染，调用后自动重置为 false。
    pub(crate) fn can_update(&self) -> bool {
        self.trigger.swap(false, Ordering::Relaxed)
    }
}
