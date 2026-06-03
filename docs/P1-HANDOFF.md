# P1 性能优化 Handoff

## 当前状态

分支 `perf/p1-performance-ux`，未 commit。8 个 Unit 代码已写完，40 个测试通过，release 编译通过。

## 已完成

- U1: macOS jwalk 3 线程 + 128KB 栈 + BufWriter 包装 stdout
- U2: 事件驱动渲染（去掉 Tick 线程，crossbeam::select! 多路复用）
- U3: Esc 取消扫描（Arc<AtomicBool> + scanner/process_read_dir 检查）
- U4: clean 规则路径合并 + 双重计算 bug 修复（Library/Caches 不再遍历 4 次）
- U5: purge 单遍遍历（消除 dir_size 串行二次遍历）
- U6: 规则级进度 [N/M]（ProgressEvent::RuleProgress）
- U7: 流式结果展示（扫描中可浏览/选择已发现的分类）
- U8: Analyzer 缓存树（完整递归树 + 索引路径导航）

## 待修复

### Bug: Analyzer 进度显示 size 累加错误
Found 事件传的是全局累加的 total_size 而非增量值，导致 found_size 指数膨胀（显示 11.07 TB）。
- 文件：`crates/tui/src/lib.rs` build_dir_tree 函数中的 Found 事件
- 修法：Found 事件的 size 字段改为增量值（本次 500 个文件的 size 增量），或者在 handle_progress 中直接用 Found 的 size 替换而非累加

### 核心体验问题: Analyzer 扫描过程中应渐进式展示目录
当前 build_dir_tree 是同步函数，全部遍历完才返回完整树。扫描 ~/ 期间用户看到空白页+数字，体验差。

**应参考 dua-cli 的做法：**
- dua 的 `BackgroundTraversal`（`dua-cli/src/traverse.rs`）在后台线程增量构建树
- 主线程通过 `crossbeam::select!` 接收 `TraversalEvent`，调用 `integrate_traversal_event()` 增量合并到树中
- 每 250ms 节流一次 UI 刷新（throttle），用户从第一秒就能看到目录和 size 在增长
- dua 源码在 `/Users/zhaohejie/workspace/explore/dua-cli`

**建议方案：**
后台线程在遍历过程中定期把"当前已知的顶层子目录 + 累加 size"作为临时 DirNode 快照推送到共享 Arc<Mutex<DirNode>>。主线程在 Scanning 状态下直接用这个快照渲染 Analyzing 视图（目录逐步出现、size 逐步增长）。扫描完成后替换为完整递归树，切换到正式 Analyzing 状态。

## 关键文件

| 文件 | 改动 |
|------|------|
| `crates/core/src/scanner.rs` | create_walker、scan_with_rules 重写、scan_purge_dir 重写、取消检查 |
| `crates/core/src/progress.rs` | RuleProgress 事件、is_cancelled() 方法 |
| `crates/tui/src/event.rs` | 去掉 Tick 线程，暴露 key_rx/progress_rx，bounded channel |
| `crates/tui/src/lib.rs` | crossbeam::select! 主循环、流式结果交互、build_dir_tree 完整递归树、索引路径导航 |
| `crates/tui/src/app.rs` | Analyzing 状态改为 tree_root+nav_path+cursor_stack、cancel_flag |
| `crates/tui/src/reporter.rs` | TuiReporter 增加 cancelled Arc<AtomicBool> |
| `crates/tui/src/ui/scan.rs` | 流式结果 UI（进度+实时 category 列表） |
| `crates/tui/src/ui/analyzer.rs` | 适配 tree_root+nav_path 数据结构 |
| `crates/cli/src/commands/clean.rs` | RuleProgress 处理 |

## 产品待定

Analyzer 功能需要产品重设计（详见 `docs/ideation/2026-06-03-p1-perf-ux-ideation.md` 的 Open Product Questions 部分）。当前定位是无差异化的 dua-cli 副本，需要重新定位为"服务于清理"的工具。
