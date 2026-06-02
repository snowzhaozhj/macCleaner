---
date: 2026-06-03
topic: macCleaner-v1
---

# macCleaner v1 Requirements

## Summary

macCleaner v1 是一个 Rust 编写的 Mac 清理工具，提供 CLI（clap）+ TUI（ratatui）两层界面，共享同一核心引擎。包含四个核心命令：clean（缓存清理）、uninstall（应用卸载）、analyze（磁盘分析）、purge（开发产物清理）。采用"智能推荐 + 一键清理"模式，文件按安全等级三级分类（safe/moderate/risky），默认只勾选安全项。所有删除操作先移到废纸篓，执行后提示用户清空。

---

## Problem Frame

Mac 用户的磁盘空间被开发缓存、应用残留、系统日志等"隐形垃圾"持续蚕食。现有清理工具要么收费昂贵（CleanMyMac 年费 $35+），要么用恐吓式营销逼迫付费，要么过度扫描带来隐私和误删风险。即便是开源替代品（如 Mole），扫描性能也差，交互体验粗糙。用户需要一个免费、透明、快速、安全的清理工具。

---

## Key Decisions

**Rust 作为核心技术栈** — Rust 的零开销并发（rayon）和无 GC 特性最适合文件系统密集型操作，二进制体积小（3-8MB），内存占用极低。CLI 用 clap，TUI 用 ratatui，均为 Rust 生态一流方案。

**CLI + TUI 先行，GUI 延后** — 主要用户是开发者，CLI + TUI 已经覆盖核心用户群。引擎先做对、做快，Tauri GUI 作为 v2 独立迭代，不需要重构核心。

**安全三级分类** — 文件按安全等级标记为 safe（缓存、日志、临时文件）/ moderate（旧下载、语言文件）/ risky（应用数据）。默认只勾选 safe 级别，宁可少清一点也不误删。

**移到废纸篓而非永久删除** — 所有删除操作默认移到 macOS 废纸篓，给用户恢复窗口。清理完成后提示是否清空废纸篓。仅在用户显式传入 `--permanent` 标志时才永久删除。

**purge 默认扫描用户主目录** — 开发产物（node_modules、target、.venv 等）绝大多数在 `~/` 下。默认扫描 `~/`，支持指定自定义路径以缩小范围、加速扫描。

---

## Actors

A1. **Mac 开发者（主要）** — 磁盘被开发产物占满，需要定期快速清理
A2. **普通 Mac 用户（次要，v1 通过 CLI 有限覆盖）** — 磁盘告警时想安全清理

---

## Requirements

**核心引擎**

R1. 引擎提供统一的扫描、分析、删除 API，CLI 和 TUI 共享同一引擎，无重复逻辑。

R2. 扫描采用并发执行（rayon 并行遍历），全盘扫描目标 < 30 秒。

R3. 每个可清理项标记安全等级：safe / moderate / risky。safe 级别的项默认勾选，moderate 和 risky 需要用户主动勾选。

R4. 所有删除操作默认移到 macOS 废纸篓（`~/.Trash`）。仅在用户传入 `--permanent` 标志时永久删除。

R5. 清理执行完成后，提示用户废纸篓当前大小，询问是否清空。

**clean 命令（缓存清理）**

R6. 扫描并清理以下类别：系统缓存（`~/Library/Caches`）、应用日志（`~/Library/Logs`）、系统临时文件（`/tmp`、`/var/folders`）、浏览器缓存（Chrome/Safari/Firefox）、已下载的邮件附件。

R7. 扫描结果按类别分组展示，每组显示文件数量和总大小。

**uninstall 命令（应用卸载）**

R8. 列出已安装的应用（`/Applications` 和 `~/Applications`），支持搜索/过滤。

R9. 卸载时自动发现并清理应用的关联文件：`~/Library/Application Support/<app>`、`~/Library/Preferences/<bundle-id>.plist`、`~/Library/Caches/<bundle-id>`、`~/Library/LaunchAgents/<bundle-id>.*`、`~/Library/Saved Application State/<bundle-id>.savedState`。

R10. 卸载前展示应用本体和所有关联文件列表及总大小，用户确认后执行。

**analyze 命令（磁盘分析）**

R11. 交互式磁盘用量浏览器：以树状结构展示目录大小，支持进入/退出子目录。

R12. 自动标记大文件（默认阈值 100MB，可通过 `--threshold` 调整）。

R13. 在 TUI 模式下提供可视化的空间占用展示（条形图或树状图）。

**purge 命令（开发产物清理）**

R14. 默认扫描 `~/` 目录，支持通过参数指定自定义扫描路径（如 `mc purge ~/workspace`）。

R15. 识别以下开发产物：`node_modules`、`target`（Rust）、`.venv` / `venv`（Python）、`__pycache__`、`dist` / `build`、`.gradle`、`DerivedData`（Xcode）、`Pods`（CocoaPods）。

R16. 按项目分组展示扫描结果，每组显示项目路径、产物类型和大小。用户可勾选/取消勾选单个项目。

**CLI 交互**

R17. 所有命令支持 `--preview` / `--dry-run` 标志：只展示将要执行的操作，不实际执行。

R18. 所有命令支持 `--yes` / `-y` 标志：跳过确认提示，直接执行（仅限 safe 级别的项）。

R19. 支持 `--json` 输出格式，方便脚本和自动化集成。

R20. 执行前默认显示分类汇总（类别、文件数、总大小）并要求确认。

**TUI 交互**

R21. TUI 提供主菜单，列出四个核心命令，用户通过键盘导航。

R22. 扫描过程显示实时进度（扫描路径、已发现项数、已统计大小）。

R23. 扫描结果以分类列表展示，支持展开/折叠到文件级别，支持全选/取消勾选。

R24. 使用颜色区分安全等级：safe（绿色）、moderate（黄色）、risky（红色）。

---

## Key Flows

F1. **一键清理流程（clean 命令）**
- **Trigger:** 用户执行 `mc clean`
- **Actors:** A1, A2
- **Steps:** 并发扫描目标目录 → 按安全等级分类 → 展示分类汇总（safe 项默认勾选）→ 用户确认 → 移到废纸篓 → 展示释放空间 → 提示是否清空废纸篓
- **Covered by:** R2, R3, R4, R5, R6, R7, R20

F2. **应用卸载流程（uninstall 命令）**
- **Trigger:** 用户执行 `mc uninstall`
- **Actors:** A1, A2
- **Steps:** 列出已安装应用 → 用户选择应用 → 扫描关联文件 → 展示应用本体 + 关联文件列表 → 用户确认 → 移到废纸篓
- **Covered by:** R8, R9, R10

F3. **磁盘分析流程（analyze 命令）**
- **Trigger:** 用户执行 `mc analyze [path]`
- **Actors:** A1
- **Steps:** 扫描指定目录（默认 `~/`）→ 构建目录大小树 → 交互式浏览 → 标记大文件 → 用户可选择删除标记项
- **Covered by:** R11, R12, R13

F4. **开发产物清理流程（purge 命令）**
- **Trigger:** 用户执行 `mc purge [path]`
- **Actors:** A1
- **Steps:** 扫描指定路径（默认 `~/`）→ 识别开发产物 → 按项目分组展示 → 用户勾选/取消 → 确认 → 移到废纸篓
- **Covered by:** R14, R15, R16

---

## Acceptance Examples

AE1. **safe 级别自动勾选**
- **Covers R3, R20.** 用户执行 `mc clean`，扫描完成后展示汇总，其中 safe 级别的缓存和日志类别已默认勾选，moderate 和 risky 类别未勾选。用户无需额外操作即可确认清理 safe 项。

AE2. **dry-run 不执行删除**
- **Covers R17.** 用户执行 `mc clean --preview`，输出与正常扫描相同的分类汇总，但不显示确认提示，不执行任何删除。退出码为 0。

AE3. **废纸篓流程**
- **Covers R4, R5.** 用户确认清理后，文件移到 `~/.Trash`，终端显示"已释放 3.2GB（已移至废纸篓）"。随后提示"废纸篓当前占用 5.1GB，是否清空？[y/N]"。

AE4. **purge 按项目分组**
- **Covers R15, R16.** 用户执行 `mc purge ~/workspace`，输出按项目分组：`~/workspace/project-a/node_modules (420MB)`、`~/workspace/project-b/target (1.2GB)`。用户可以取消勾选 project-a，只清理 project-b。

---

## Scope Boundaries

**Deferred for later (v2+):**
- GUI 界面（Tauri + Web 前端）
- optimize 命令（系统维护：重建 Spotlight/Launch Services 索引）
- status 命令（实时系统监控仪表盘）
- installer 命令（扫描大体积安装包 .dmg/.pkg/.zip）
- 增量扫描缓存（记住上次扫描结果，只扫描变化部分）
- 跨平台支持

**Outside this product's identity:**
- 杀毒/恶意软件扫描——这不是安全工具
- 系统性能优化/加速——清理工具不是性能调优工具
- 遥测/数据收集——零遥测是核心承诺
- 付费功能/订阅——完全免费和开源

---

## Dependencies / Assumptions

- macOS 的 Full Disk Access 权限：clean 和 uninstall 命令需要访问 `~/Library` 下的受保护目录。应用需要引导用户在系统偏好设置中授予该权限。
- 废纸篓操作依赖 macOS 的 `NSFileManager` 或 `trash` 命令行工具，需要确认 Rust 中的调用方式（通过 `osascript` 或 `objc2` crate）。
- Homebrew 作为主要分发渠道，需要创建 Homebrew formula。

---

## Sources / Research

- [tw93/Mole](https://github.com/tw93/Mole) — Shell + Go 的开源 Mac 清理工具，提供 clean/uninstall/analyze/purge 等功能，扫描性能较差
- CleanMyMac X / CCleaner / BleachBit / AppCleaner 的 UX 模式调研：两步确认、分类展示、安全分级、废纸篓优先
- Rust 生态：clap（CLI）、ratatui（TUI）、rayon（并发）、walkdir（文件遍历）均为成熟的一流库
