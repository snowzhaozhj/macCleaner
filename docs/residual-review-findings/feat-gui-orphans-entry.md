# Residual Review Findings

来源：GUI 孤儿残留入口代码评审（分支 `feat/gui-orphans-entry`，基线 `4041ec846253f8aadeb179fef9b413a27ebb4fdd`）。

- **P3** `crates/gui/src/commands/orphans.rs:42` — 并发 `scan_orphans` 请求乱序完成时，较旧快照可能覆盖 `last_orphans` 中较新的删除权威快照。建议后续引入 Orphans 专用单调请求代次，仅允许最新请求写槽，并增加 A→B 启动、B→A 完成的回归测试。当前后果偏向静默少删而非越权多删；本 PR 保持 `preselect=false`、可信槽回查、后端授权与 Trash-only，因此不在机械评审修复中扩大异步协议。
  - **✅ 已解决**（2026-07-20，`docs/plans/2026-07-20-001-fix-gui-scan-slot-race-plan.md`）：护栏上提到 `AppState` 共享祖先，`VersionedSlot<T>` 代次守卫写槽统一覆盖五个扫描槽（不止 orphans）。判等与写槽在单锁内原子完成，回归契约见 `crates/gui/src/slot.rs` 测试模块。
