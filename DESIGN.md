# Design

macCleaner 的视觉系统。当前的唯一实现是 **TUI（ratatui + crossterm）**；本文既**如实记录 TUI 现状**，又把每个视觉决策抽象为**框架无关的语义 token**，作为未来桌面端（GUI）的移植基准。核心不变量：安全等级三通道编码（颜色 + 形状 + 文字）、无静默删除、危险色稀缺化。

> 阅读顺序：先看 §1 语义 token 层（唯一事实来源）→ §2 后端映射表（TUI 现状 + 桌面基准）→ 其后是排版/布局/组件/动效/可访问性。改视觉时改 token，不要在各 UI 文件里散落硬编码 `Color::`。

---

## 1. 语义 Token 层（唯一事实来源）

所有颜色以**语义角色**命名，与渲染后端解耦。TUI 当前把它们实现为 16 色 ANSI；桌面端把同一角色实现为 OKLCH。角色不变，值随后端变。

### 1.1 安全轴（产品的招牌视觉语言）

安全等级是本产品最重要的视觉信号，驱动配色 + 形状 + 标签三通道。三个值来自 `mc_core::models::SafetyLevel`，语义见 CONCEPTS.md。

| Token | 含义 | 形状字形 | 文字标签 |
|---|---|---|---|
| `safety.safe` | 零数据丢失、自动透明补回 | `●` | 安全 |
| `safety.moderate` | 零数据丢失、但重建需用户发起且有耗时 | `▲` | 中等 |
| `safety.risky` | 可能丢失不可再生数据/有价值状态 | `✕` | 危险 |

**不变量**：这三者永远同时输出 颜色 + 形状 + 标签。任何后端都不得退化为纯色块。

### 1.2 状态轴（反馈与操作语义）

| Token | 用途 |
|---|---|
| `state.success` | 已释放/总大小/完成/正向数量强调 |
| `state.activity` | 进行中：扫描、排序、清理、spinner、状态提示条 |
| `state.warning` | 警示（非进行中）：确认标题、隐藏项提示、大文件高亮、type-to-confirm 输入 |
| `state.danger` | 破坏性动作：删除按钮、待删标记、取消 |
| `state.info` | 中性强调：计数、选中、可交互高亮（TUI 中 = `accent`） |

> **重要的语义重叠（桌面端要显式处理的决策点）**：TUI 中 `safety.safe` 与 `state.success` 都是绿、`safety.moderate`/`state.activity`/`state.warning` 都是黄、`safety.risky` 与 `state.danger` 都是红——因为 ANSI 只有 8 个基础色，被迫共用。`state.activity`（进行中）与 `state.warning`（警示）在终端同为黄，但语义不同，桌面端应区分（如进行中用中性/动效、警示用琥珀）。这在终端里可接受（用户靠上下文区分），但**桌面端色彩空间充足，应把"安全等级"与"操作状态"在同一色相家族内用明度/彩度区分**（例：safety 点用低彩度稳态绿，success 数字用高明度绿），既保留"绿=好"的习得联想，又不让二者是同一个色块。见 §2 备注。

### 1.3 结构轴（中性层）

| Token | 用途 |
|---|---|
| `ink.primary` | 主前景：默认文字、激活项、文件名 |
| `ink.muted` | 次要信息：标签、大小/百分比列、描述、页脚、分隔符 |
| `accent` | 主交互色：选中、焦点、面板边框、计数强调、面包屑 |
| `accent.explore` | 分析器（Analyze 模式）专属次强调：列表边框 + 体积条 |
| `surface.base` | 主背景 |
| `surface.overlay` | 对话框/浮层背景 |
| `border.subtle` | 非强调边框（详情面板等） |
| `cursor` | 光标行高亮 |

---

## 2. 后端映射表

一行一个语义 token：左列是**当前 TUI（ANSI）**实现（源码事实），右列是**桌面端基准（OKLCH）**——暗色优先（见 §3 主题决策）。桌面值是经过对比度校验的**起点提案**，落地时再微调，但**色相家族须锚定 TUI 现状以保持产品识别**。

| 语义 Token | TUI (ANSI) | 桌面基准 (OKLCH, 暗色) | 备注 |
|---|---|---|---|
| `safety.safe` | `Green` `●` | `oklch(0.70 0.10 150)` `●` | 稳态、低彩度，作状态点 |
| `safety.moderate` | `Yellow` `▲` | `oklch(0.78 0.11 90)` `▲` | |
| `safety.risky` | `Red` `✕` | `oklch(0.62 0.18 25)` `✕` | 稀缺使用；见原则 1 |
| `state.success` | `Green` + BOLD | `oklch(0.76 0.15 150)` | 同 safe 色相、更亮更艳以区分 |
| `state.activity` | `Yellow` + BOLD | `oklch(0.80 0.13 90)` | spinner/进度 |
| `state.warning` | `Yellow` + BOLD | `oklch(0.82 0.14 75)` | 警示，桌面端偏琥珀以别于 activity |
| `state.danger` | `Red` + BOLD | `oklch(0.60 0.20 25)` | 删除动作、待删标记 |
| `state.info` | `Cyan` + BOLD | `oklch(0.75 0.11 210)` | = accent |
| `ink.primary` | `White` | `oklch(0.93 0.005 250)` | 正文，对比 ≥ 12:1 |
| `ink.muted` | `DarkGray` | `oklch(0.68 0.010 250)` | 仅次要信息；对比 ≥ 4.5:1 |
| `ink.faint` | （TUI 未区分） | `oklch(0.52 0.010 250)` | 桌面新增：占位/禁用 |
| `accent` | `Cyan` | `oklch(0.75 0.11 210)` | 边框/选中/焦点 |
| `accent.explore` | `Blue` | `oklch(0.66 0.13 250)` | 分析器边框 + 体积条 |
| `surface.base` | 终端默认 (Reset) | `oklch(0.18 0.008 250)` | 近黑、微冷 |
| `surface.raised` | （TUI 无层级） | `oklch(0.22 0.010 250)` | 桌面面板层 |
| `surface.overlay` | Clear→默认 | `oklch(0.25 0.012 250)` | 模态浮层 |
| `border.subtle` | `DarkGray` | `oklch(0.34 0.010 250)` | 详情面板边框 |
| `cursor` | `DarkGray` bg / 无色时 `REVERSED` | `oklch(0.30 0.012 250)` bg | 见可访问性 |
| `toast.fg` / `toast.bg` | `Black` / `Yellow` bg + BOLD | `oklch(0.20 0 0)` / `state.activity` | 状态提示条反色 |

**门控（两个后端都遵守）**：
- **NO_COLOR**（TUI）：`theme::c()` 把任何前景色回退为终端默认，光标高亮改用 `REVERSED`。桌面端对应"高对比/单色模式"须保留同样降级路径。
- **对比度**：`ink.primary` 与 `ink.muted` 在 `surface.base` 上分别 ≥ 12:1 与 ≥ 4.5:1；`ink.muted` 不得承载正文，只承载可丢失的次要信息（大小、百分比、描述）。

---

## 3. 主题（Theme）

**暗色优先。** 场景句：一个开发者，深夜在深色 IDE 旁的终端里，磁盘告警，想快速且放心地清出几十 GB——这个场景强制暗色。工具是终端原生的（btop/lazygit 谱系），亮色会与宿主终端割裂。

- **TUI**：不画背景，继承用户终端配色（暗/亮自适应用户环境），只画前景 + 边框。这是终端工具的正确做法，保持与宿主一致。
- **桌面端**：以 §2 暗色基准为默认主题；亮色主题作为后续变体（同色相、翻转明度阶：`surface.base` → `oklch(0.98 …)`，`ink.primary` → `oklch(0.25 …)`，安全/状态色降明度提彩度以在浅底上达标）。

配色策略（impeccable 轴）：**Restrained**——中性层 + 一个主 accent（cyan），安全/状态色只在语义位置出现，绝不装饰。危险红是全局最稀缺的颜色。

---

## 4. 排版（Typography）

**TUI**：等宽单元格网格，字体由终端决定，无字号概念。"层级"由三种手段构成，而非字号：

1. **粗细** —— `Modifier::BOLD` 承担所有强调（菜单标题/选中项、页头计数与大小、面包屑末段、状态条、帮助键、确认标题/按钮）。
2. **颜色角色** —— 见 §1。
3. **对齐与列宽** —— 定宽列（如分析器 `name {:<24}`、`size {:>8}`、`percent {:>3}%`）形成表格感的纵向节奏。

**桌面基准**：
- **一个等宽家族**（SF Mono / JetBrains Mono / ui-monospace 栈）承载文件名、大小、路径——保留终端工具的数据对齐感与识别度。UI 标签（按钮/标题/菜单）可用系统 UI 无衬线（SF Pro / system-ui）以提可读，但**数据列必须等宽**。这是本产品的排版签名，勿丢。
- 固定 rem 阶（product register 惯例），比例 1.125–1.2；不用流体 clamp。
- 权重层级映射 TUI：regular = `ink.primary` 正文，medium/semibold = 现在的 BOLD 强调位。
- 长路径/正文按 65–75ch 截断或换行；数据表可放宽到 120ch+。

---

## 5. 布局（Layout）

### 5.1 三行外壳（所有主屏共享）

TUI 的规范骨架 `chrome::three_row_layout`：`[Length(3) 页头, Min(1) 主体, Length(1) 页脚]`。

- **页头**（带边框，`accent` 色）：左 = 上下文（命令名/面包屑/扫描路径），右对齐 = 汇总统计（计数、总大小、已选/已标记）。左右子区按显示宽度分割，不重叠。
- **主体**：列表 / 分析视图 / 详情分栏。
- **页脚**（无边框，`ink.muted`）：快捷键提示条，或过滤输入态。**拥挤时按优先级降级**——宽度不足时按 `KeyHint.priority`（0=最高，永不剔除）**整条剔除**低优先项，绝不留半截文字；`x 删除` 与 `? 帮助` 是两条硬保留（priority 0），保证任何宽度下可见（`keymap::footer_line`）。

**桌面映射**：页头 → 标题栏/工具栏（左标题右统计）；主体 → 内容区；页脚 → 状态栏。响应式是**结构性**的（窄窗折叠详情面板、面包屑省略中段），不是流体字号。

### 5.2 各屏约束（现状记录）

| 屏 | 约束 | 说明 |
|---|---|---|
| Menu | `[Length(5), Min(10), Length(3)]` | 标题 / 操作列表 / 页脚 |
| Scan / Results / Analyze | `[Length(3), Min(1), Length(1)]` | 共享三行壳 |
| Results 主体再分栏 | `[Min(3), Length(5)]` | 列表 + 详情面板（`DETAIL_HEIGHT=5`：路径/影响/恢复三行；`body.height ≤ 8` 时折叠） |
| Cleaning | `[Length(3), Min(5), Length(3)]` | 标题 / 进度 / 页脚 |
| Done / Sorting | `[Min(3), Length(3)]` | 内容 / 页脚 |

### 5.3 浮层

居中浮层：**帮助**宽度自适应 `min(最长行显示宽 + 4, 90% 屏宽)`（取代固定 48%，窄终端下会截断）；**确认删除** 80% 宽、高度 `min(内容, 屏高−2)`，内部三段 `[Length 头, Min(1) 可滚动清单, Length 钉底操作区]`——操作指引行钉底、任何尺寸永不被裁。桌面映射为居中模态——但遵循 product 原则：**模态是最后手段**，能内联/渐进就不弹窗（确认删除因不可逆而正当，帮助浮层可考虑改常驻侧栏）。

间距节奏：TUI 靠单元格与定宽列。桌面基准建立 4px 基准的间距阶（4/8/12/16/24/32），列表行高 ≥ 28px 保证可点。

---

## 6. 组件清单（Components）

当前 TUI 已实现的组件，及其状态/样式契约。桌面端须为每个交互组件补齐 default / hover / focus / active / disabled / loading 全套（TUI 只有 default + cursor/selected）。

| 组件 | 源 | 关键样式 | 桌面补齐点 |
|---|---|---|---|
| 页头 chrome | `ui/chrome.rs` | `accent` 边框，左上下文/右统计 | — |
| 页脚快捷键条 | `ui/chrome.rs`·`keymap.rs` | `ink.muted`，无边框；拥挤时按 `priority` 整条降级（保 `x`/`?`） | 桌面改为状态栏 + 可点击操作 |
| 列表行（分隔/分类/文件） | `ui/rows.rs` | 安全色+`●▲✕`+`[x]/[-]/[ ]`复选+`▼/▶`展开+大小 | hover 态、拖选、右键菜单 |
| 详情面板 | `ui/results.rs` | `border.subtle` 边框，三通道头 + rubric/impact/recovery，`Wrap` | — |
| 滚动条 | `ui/chrome.rs` | `VerticalRight`，仅 `total>visible` 时显示，内缩 1 行 | 原生滚动 |
| 菜单 | `ui/menu.rs` | `▶ ` 选中标记，`accent`+BOLD；描述 `ink.muted` | 卡片/列表可选，避免同尺寸卡片堆砌 |
| 进度屏 | `ui/scan.rs` | spinner(`state.activity`) + 路径 + 实时计数/大小，**无 Gauge** | 可加确定性进度条（若可得总量） |
| 确认删除对话框 | `ui/confirm.rs` | 固定头+可滚动清单+钉底操作区；Risky 置顶全量(含 wrap 的 impact/recovery)、非 Risky 分类汇总+最大 Top-3；含未勾选子项时 ⚠ 披露；`state.danger` 边框；type-to-confirm；"移废纸篓"注脚 | 保留 type-to-confirm 门槛 |
| 帮助浮层 | `ui/mod.rs` | 键 `accent`+BOLD / 描述 `ink.primary` | 考虑常驻而非模态 |
| 状态提示条(toast) | `ui/mod.rs` | 底部 1 行，`toast.fg/bg` 反色+BOLD | 定时淡出 |
| 分析器行（体积条） | `ui/analyzer.rs` | `█/░` 条(`accent.explore`)，marked→`state.danger`+`CROSSED_OUT`，**大文件**(>100MiB **且为文件**)→`state.warning`+`⚠ ` 前缀，普通文件→`ink`，目录→`accent`(恒定，不受体积高亮) | 真图形条 |

**符号表（须跨后端保留识别度）**：安全 `●▲✕`；复选 `[x] [-] [ ]`；展开 `▼ ▶`；菜单游标 `▶`；分析器体积条 `█ ░`、待删 `[D]`；面包屑分隔 ` / `；spinner `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`；警告 `⚠`；输入光标 `▏`。

---

## 7. 动效（Motion）

**TUI 现状**：唯一动效是 braille spinner（`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`，按 tick 轮转），用于扫描/清理/排序进行态。无渐变、无过场——符合原则"进度只传达状态，不表演"，也直接反对 CleanMyMac 式表演式拖延。

**桌面基准**：
- 过渡 150–250ms，`ease-out`（quart/quint），无 bounce/elastic。
- 动效只传达状态：状态切换、加载、反馈、揭示——绝不装饰。无编排式加载序列（product 载入即入任务）。
- spinner → 不确定进度指示器；若能拿到扫描总量，升级为确定性进度条。
- **`prefers-reduced-motion` 必备降级**：spinner → 静态"扫描中"文字，过渡 → 瞬切。

---

## 8. 可访问性（Accessibility）

这是本产品的一等约束，不是补丁：

1. **三通道编码**（颜色 + `●▲✕` 形状 + 中文标签）——安全等级永不靠单一颜色。桌面端同样：状态点必须带形状/图标 + 文字，不能只是色块。
2. **NO_COLOR / 高对比模式**：TUI 走 `theme::c()` 回退 + `REVERSED` 高亮；桌面须提供等效单色/高对比主题。
3. **对比度门槛**：正文 ≥ 4.5:1，大字/粗体 ≥ 3:1，`ink.muted` 只放次要信息并逐项校验（见 §2 门控）。
4. **减少动效**：§7 降级路径。
5. **键盘优先**：TUI 全键盘（`ui/keymap.rs`）。桌面端键盘可达性须与鼠标等价——每个鼠标动作都有快捷键。

---

## 9. 桌面移植检查清单（Baseline Handoff）

未来做 GUI 时，按此表逐项落地，即可保持与 TUI 同源识别：

- [ ] 实现 §1 语义 token 为主题结构体/CSS 变量，值取 §2 桌面基准列
- [ ] 安全等级三通道（色 + 形状图标 + 文字）——**先实现这个，它是识别核心**
- [ ] 在同色相家族内区分 safety 与 state（§1.2 重叠决策）
- [ ] 数据列用等宽字体，UI 标签可用系统无衬线（§4）
- [ ] 三行外壳 → 标题栏/内容/状态栏（§5.1）
- [ ] 每个交互组件补齐 6 态；保留 type-to-confirm 门槛（§6）
- [ ] 危险红稀缺化——不做 CleanMyMac 式红色轰炸（PRODUCT.md 反例）
- [ ] NO_COLOR / 高对比 / reduced-motion 三条降级（§7–8）
- [ ] 对比度全量校验（§2 门控）

> 视觉决策沿革散见 `docs/ideation/2026-07-04-tui-ux-maturity.md`、`docs/brainstorms/2026-06-05-tui-ux-overhaul-requirements.md`；安全模型 rubric 见 `crates/core/src/models.rs` 的 `SafetyLevel` 文档注释。
