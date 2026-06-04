---
title: Throttle 门控渲染 + 静态进度显示
status: active
date: 2026-06-04
type: requirements
---

# Throttle 门控渲染 + 静态进度显示

## Summary

引入 dua-cli 式 Throttle（250ms AtomicBool 定时器）控制所有扫描模式的 UI 刷新频率，同时将进度区的快速闪烁路径文本替换为稳定的统计信息，彻底消除 header 闪烁。

## Problem

当前所有扫描模式（Clean/Purge/Analyze）的 `ProgressEvent::Scanning { path }` 事件每 500 个文件触发一次 `progress_text` 更新。在高速遍历时（jwalk 3 线程），路径文本以人眼不可读的频率变化，直接导致 header 区域视觉闪烁。

## Outcomes

R1 扫描期间 header 区域视觉稳定，无闪烁
R2 用户仍能感知"扫描在进行"（spinner + 数字在变）
R3 键盘操作零延迟响应（不受 throttle 节制）
R4 对所有扫描模式（Clean/Purge/Analyze）统一生效

## Approach

引入 `Throttle` struct（独立后台线程 + AtomicBool），控制数据更新何时触发 `terminal.draw()`。进度区内容改为低频变化的统计数字。

## Behavior

### Throttle 机制

- 独立线程每 250ms 将 `AtomicBool` 设为 true
- 主循环收到 progress 事件后正常更新 App 状态，但**不立即重绘**
- 只有当 `throttle.can_update()` 返回 true 时才调用 `terminal.draw()`
- `can_update()` 实现：`self.trigger.swap(false, Relaxed)` — 原子读并重置
- 当 Throttle struct 被 drop 时，后台线程自动退出（Weak 引用检测）

### 键盘输入绕过 throttle

- 键盘事件处理后**立即**触发 `terminal.draw()`，不等 throttle
- 保证用户按 j/k/Enter 等操作后即时看到视觉反馈

### 静态进度显示

- 进度区内容：`已扫描 N 个文件 | X GB | 当前: <顶层目录名>`
- 顶层目录名仅在切换时更新（频率 < 1Hz）
- spinner 动画仍由现有 100ms 超时驱动（不变）

### 作用范围

- Clean 扫描、Purge 扫描、Analyze 扫描全部适用
- Throttle 在进入 Scanning 状态时创建，离开时 drop

## Non-goals

- 不改变状态机结构（AppState 枚举不动）
- 不改增量树逻辑
- 不改键盘交互行为

## Success criteria

- 在 ~/ 全量 Analyze 扫描期间，header 区域无可感知闪烁
- 键盘响应延迟 < 16ms（即时重绘）
- 编译通过 + 40 个测试通过
- `/verify-tui analyze` 验证渐进式预览正常 + 无闪烁

## Reference

- dua-cli `Throttle` struct: `dua-cli/src/common.rs` L129-172
- dua-cli 使用位置: `dua-cli/src/traverse.rs` L290, L449
