---
name: macCleaner
last_updated: 2026-06-03
---

# macCleaner Strategy

## Target problem

Mac 用户的磁盘空间被开发缓存、应用残留、系统日志等"隐形垃圾"持续蚕食，但现有清理工具要么收费昂贵（CleanMyMac 年费 $35+），要么用恐吓式营销逼迫付费，要么过度扫描带来隐私和误删风险。即便是开源替代品（如 Mole），扫描性能也很差，交互体验粗糙——用户想要一个"干净地清理垃圾"的工具，却找不到一个既免费、透明、快速、又不会搞坏系统的选择。

## Our approach

完全开源、零遥测、零订阅，同时在性能和交互上不妥协——扫描要快（并发 + 增量），预览要清晰（每个将删除的文件和预估空间一目了然），操作要安全（无静默删除）。通过 CLI/TUI/GUI 三层界面适配不同使用习惯，让清理工具本身也保持轻量，不成为新的"系统负担"。

## Who it's for

**Primary:** Mac 开发者 - 磁盘被 Xcode DerivedData、node_modules、Docker 镜像、brew 缓存等开发产物持续占满，需要一个快速、安全、可信的工具定期释放空间。

**Secondary:** 普通 Mac 用户 - 磁盘告警时想找到占空间的东西并安全清理，但不想为此付费或安装一个比问题更大的工具。

## Key metrics

- **扫描耗时** - 全盘扫描完成所需秒数；本地计时，目标 < 30s
- **单次清理释放空间中位数** - 每次清理实际释放的 GB 数中位值；本地统计
- **GitHub Stars 增长率** - 月新增 Stars 数；GitHub Insights
- **Issue 解决率** - 已关闭 Issue / 总 Issue；GitHub Issues

## Tracks

### 核心清理引擎

安全、高性能的扫描、分析和删除能力——并发扫描、增量缓存，支持缓存清理、应用卸载残留、系统日志、大文件发现等。所有操作预览后执行，无静默删除。

_Why it serves the approach:_ 快速 + 透明可审查的清理是产品区别于竞品和现有开源方案的根基。

### 多界面适配

CLI（脚本/自动化友好）、TUI（终端交互）、GUI（普通用户友好）三层界面共享同一引擎。注重交互友好性——清晰的进度反馈、直观的分类展示、舒适的操作流程。

_Why it serves the approach:_ 让不同技术背景的用户都能用自己舒适的方式完成清理，良好的交互体验是用户愿意持续使用的关键。

### 开发者专项清理

针对 Xcode DerivedData、node_modules、Docker、brew 缓存、Python venv 等开发产物的智能识别和批量清理。

_Why it serves the approach:_ 开发者是主要用户，这是 CleanMyMac 等竞品覆盖不到或做不好的领域。
