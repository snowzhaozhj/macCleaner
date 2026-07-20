//! `VersionedSlot<T>`：代次守卫写槽原语。
//!
//! 五个扫描结果槽（`last_scan`/`last_purge`/`last_uninstall`/`last_analyze`/
//! `last_orphans`）共享同一「扫完写槽、删除按槽取项」模式。当同一 tab 内两次扫描
//! 请求乱序完成（慢的先发、后完成）时，较旧快照会覆盖较新快照，此后槽是过时的删除
//! 权威源——见 `docs/plans/2026-07-20-001-fix-gui-scan-slot-race-plan.md`。
//!
//! 护栏上提到 `AppState` 共享祖先（[[per-component-guards-miss-cross-surface-races]]）：
//! 每个槽自带单调代次，扫描入口 `begin` 领代次，完成时 `commit` **仅当自己仍是最新代次**
//! 才写槽，旧代次的写入被丢弃。
//!
//! **代次判等与写槽必须在同一临界区原子完成**（评审 feasibility-P1）：若判等与写槽
//! 分处两个同步点，A（gen=1）判等通过后、抢到锁前，B（gen=2）可能整段 begin+commit
//! 完成并写入，随后 A 覆盖上去——原语自身复现了它要根治的乱序覆盖。故把代次并入
//! value 的 `Mutex`（`Mutex<(u64, Option<T>)>`），锁内比对锁里存的当前代次再写。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// 一次扫描发起时领取的代次凭据。轻量值类型，不持锁。
#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    gen: u64,
}

/// 代次守卫的结果槽。`inner` 锁内元组 = (当前权威代次, 结果)；`next_gen` 仅供 `begin` 领号。
///
/// `Clone` 克隆两个 `Arc`，供命令入口在进 `spawn_blocking` 前取 owned 句柄
/// （KTD-5：async 命令不可持有 `State<'_,_>` 借用），同既有 `.clone()` 模式。
pub struct VersionedSlot<T> {
    inner: Arc<Mutex<(u64, Option<T>)>>,
    next_gen: Arc<AtomicU64>,
}

impl<T> Clone for VersionedSlot<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            next_gen: self.next_gen.clone(),
        }
    }
}

impl<T> Default for VersionedSlot<T> {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new((0, None))),
            next_gen: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl<T> VersionedSlot<T> {
    /// 领取单调递增代次号（从 1 起，与 HTD mermaid 一致）。必须在 `spawn_blocking`
    /// **之前**调用，代次才能反映本命令的发起次序。
    pub fn begin(&self) -> Ticket {
        let gen = self.next_gen.fetch_add(1, Ordering::Relaxed) + 1;
        Ticket { gen }
    }

    /// 判等与写槽在同一临界区原子完成：仅当 `ticket.gen` 不比锁内当前代次旧才写入。
    ///
    /// 用 `<` 而非 `!=`：更晚 `begin` 的 ticket 代次更大、恒可提交；只有严格更旧的被拒
    /// （`Ok(false)`）。锁毒化返回 `Err("状态锁毒化")`（全 commands 一致文案）。
    pub fn commit(&self, ticket: Ticket, value: T) -> Result<bool, String> {
        let mut guard = self.inner.lock().map_err(|_| "状态锁毒化".to_string())?;
        if ticket.gen < guard.0 {
            return Ok(false);
        }
        guard.0 = ticket.gen;
        guard.1 = Some(value);
        Ok(true)
    }

    /// 删除端读槽：返回锁 guard，调用方按 `guard.1.as_ref()` 取结果（元组第二元）。
    /// 短临界区取 owned 项后即 drop，逻辑与既有 `last_*.lock()` 一致。
    pub fn read(&self) -> Result<MutexGuard<'_, (u64, Option<T>)>, String> {
        self.inner.lock().map_err(|_| "状态锁毒化".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 单次 begin→commit：写入成功，read 得该值。
    #[test]
    fn single_begin_commit_writes_value() {
        let slot: VersionedSlot<i32> = VersionedSlot::default();
        let t = slot.begin();
        assert_eq!(slot.commit(t, 42), Ok(true));
        assert_eq!(slot.read().unwrap().1, Some(42));
    }

    /// R1 核心断言：乱序两 ticket——新 ticket 先 commit 后，旧 ticket 的 commit 被拒。
    #[test]
    fn stale_ticket_does_not_overwrite_newer() {
        let slot: VersionedSlot<&str> = VersionedSlot::default();
        let t1 = slot.begin();
        let t2 = slot.begin();
        assert_eq!(slot.commit(t2, "B"), Ok(true));
        assert_eq!(slot.commit(t1, "A"), Ok(false), "旧代次不得覆盖新结果");
        assert_eq!(slot.read().unwrap().1, Some("B"));
    }

    /// 锁内原子性（feasibility-P1）：commit 以「锁内当前代次」而非「begin 时快照」判定。
    /// 先 begin t1、t2，先 commit(t2) 使锁内当前代次=2，再 commit(t1) 必被拒——
    /// 模拟「A 判等本应通过但 B 抢先写入」后 A 仍不得覆盖。
    #[test]
    fn commit_checks_current_generation_inside_lock() {
        let slot: VersionedSlot<&str> = VersionedSlot::default();
        let t1 = slot.begin(); // gen=1
        let t2 = slot.begin(); // gen=2
        // B 抢先：锁内当前代次推到 2
        assert_eq!(slot.commit(t2, "B"), Ok(true));
        // A 的 gen=1 严格小于锁内当前 2 → 拒绝，旧结果永不覆盖新结果
        assert_eq!(slot.commit(t1, "A"), Ok(false));
        assert_eq!(slot.read().unwrap().1, Some("B"));
    }

    /// 同代次可提交（`<` 语义）：单调最新 ticket 恒放行。
    #[test]
    fn newest_ticket_always_commits() {
        let slot: VersionedSlot<i32> = VersionedSlot::default();
        let t1 = slot.begin();
        assert_eq!(slot.commit(t1, 1), Ok(true));
        let t2 = slot.begin();
        assert_eq!(slot.commit(t2, 2), Ok(true), "更晚的 ticket 恒可提交");
        assert_eq!(slot.read().unwrap().1, Some(2));
    }

    /// 代次单调：连续 begin 得严格递增的 gen，从 1 起。
    #[test]
    fn generations_are_strictly_monotonic() {
        let slot: VersionedSlot<()> = VersionedSlot::default();
        let g1 = slot.begin().gen;
        let g2 = slot.begin().gen;
        let g3 = slot.begin().gen;
        assert_eq!((g1, g2, g3), (1, 2, 3));
    }

    /// R2 per-slot 隔离：两个独立槽的 begin 互不影响对方代次。
    #[test]
    fn slots_have_independent_generations() {
        let a: VersionedSlot<()> = VersionedSlot::default();
        let b: VersionedSlot<()> = VersionedSlot::default();
        let _ = a.begin(); // a: gen 1
        let _ = a.begin(); // a: gen 2
        assert_eq!(b.begin().gen, 1, "b 的代次不受 a 的 begin 影响");
    }

    /// 锁毒化：inner 锁毒化后 commit/read 返回 Err("状态锁毒化")。
    #[test]
    fn poisoned_lock_degrades_gracefully() {
        let slot: VersionedSlot<i32> = VersionedSlot::default();
        let slot2 = slot.clone();
        let _ = std::panic::catch_unwind(|| {
            let _g = slot2.read().unwrap();
            panic!("毒化 inner 锁");
        });
        let t = slot.begin();
        assert_eq!(slot.commit(t, 1), Err("状态锁毒化".to_string()));
        assert!(slot.read().is_err());
    }
}
