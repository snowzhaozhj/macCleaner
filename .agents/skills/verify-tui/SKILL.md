---
name: verify-tui
description: "Verify TUI rendering and interaction by running the app in tmux and inspecting captured output. Use when modifying ratatui/crossterm code, after TUI changes, before committing UI work, or when the user says 'verify', 'test the TUI', 'check the interface', 'run it', 'does it look right'. Not for non-TUI or CLI-only verification."
argument-hint: "[menu | analyze | clean | purge | all]"
---

Run the TUI app in tmux, simulate user interaction, capture pane output, and verify correctness. Report PASS/FAIL per scenario with evidence.

## Execution

1. Build release: `cargo build --release`. Stop on failure.
2. Kill stale session: `tmux kill-session -t verify-tui 2>/dev/null`
3. Launch: `tmux new-session -d -s verify-tui -x 120 -y 35 "./target/release/mc"`
4. Wait 2s for startup.
5. Run scenarios (see below). Default to `all` if no argument given.
6. Cleanup: `tmux kill-session -t verify-tui`
7. Report results.

## Capture pattern

```bash
tmux capture-pane -t verify-tui -p
```

Use `until <grep condition>; do sleep 2; done` (max 120s) to wait for async state transitions. Do NOT use raw `sleep` for waiting.

## Scenarios

### menu

Capture immediately after startup. Assert:
- "macCleaner" title present
- Four options: Clean, Uninstall, Analyze, Purge
- Bottom hint contains "↑↓ 选择"

### analyze

1. Navigate: `tmux send-keys -t verify-tui Down Down Enter`
2. Wait 3s, capture. Assert **渐进式预览**:
   - "磁盘分析中..." with spinner
   - "目录 (实时," header with item count
   - At least 3 directory entries with size and bar chart
3. Wait for completion: grep "↑↓/jk 移动"
4. Capture. Assert **完整 Analyzing 视图**:
   - "总大小:" shows a reasonable value (< 5 TB)
   - "文件列表 (按大小排序)" with multiple entries
   - Breadcrumb navigation shows root name
5. Test drill-down: `send-keys Enter`, wait 0.5s, capture. Assert breadcrumb gained a segment.
6. Return: `send-keys q`, wait 0.5s.

### clean

1. From menu: `send-keys Enter` (cursor defaults to Clean)
2. Wait 3s, capture. Assert:
   - "系统缓存扫描 中..." title
   - "已发现:" counter updating
3. Wait for results or empty: grep "已发现.*文件\|未发现可清理"
4. Esc back to menu.

### purge

1. Navigate: `send-keys Down Down Down Enter`
2. Wait 3s, capture. Assert scanning state visible.
3. Esc back to menu.

## Between scenarios

Always verify you're back at Menu (grep "选择操作") before starting the next scenario. If not, send `q` or `Escape` until menu appears.

## Failure criteria

- Size display > 5 TB → likely accumulation bug
- Missing expected UI elements after timeout → rendering regression
- Garbled characters in bar chart area → encoding issue
- App crashes (tmux session dies) → panic or fatal error

## Reporting

For each scenario output one line:
```
[PASS/FAIL] scenario_name — observation
```

If FAIL, include the captured pane content for diagnosis.
