---
date: 2026-07-04
topic: tui-ux-maturity
focus: 优化 TUI 交互体验，对齐 dua-cli，达到成熟体验
mode: repo-grounded
grounding: 编排者亲自在 tmux 上手体验 mc 与 dua 全流程 + dua-cli 源码 + 6 帧 ideation
---

# Ideation: TUI 交互体验成熟度（对齐 dua-cli）

> 用户诉求："现在的 TUI 体验太差了，和 dua 对齐，达到成熟的体验。你自己多体验体验，测试测试，不仅仅测试功能，要测试交互体验。"
> 本文的 grounding 不是纸面推演——编排者在 tmux 里逐个跑了 mc 的 Menu/Analyze(实时+完成+钻取+标记)/Clean(结果+展开+确认)/Uninstall，又逐屏跑了 `dua i` 并翻完其 help 覆盖层，据此提炼差距。

## Grounding Context

**项目形态:** Rust workspace — `crates/core`(引擎) + `crates/cli`(bin `mc`) + `crates/tui`。ratatui 0.29 + crossterm 0.28 + crossbeam-channel。`unsafe_code = deny`，clippy pedantic。

**TUI 架构现状:**
- 状态机 `AppState`(app.rs)：Menu / Scanning / Results / Confirming / Cleaning / Done / Analyzing / AnalyzingLive / Sorting（9 态）。
- 主循环 lib.rs：动画态用 `crossbeam::Select` 多路复用键盘+进度事件，静态态纯阻塞等待；已有 `throttle.rs`(200ms 门控渲染)。event.rs 独立线程 poll 键盘。
- 渲染 ui/{menu,scan,results,confirm,analyzer}.rs——**每状态各写一套 Layout + hint 行 + 边框盒 + spinner**（`spinner_char`/`SystemTime::now()` 在 scan.rs、analyzer.rs 重复 4+ 处）。
- 键盘处理分散在 lib.rs 的 6 个 `handle_*_key`，各自重写方向键/退出键。
- **两套并存的"标记"机制**：Results 用 `ScanItem.selected`(分类勾选)；Analyze 用 `marked_for_delete: HashSet<PathBuf>`。
- 已落地(近一月)：单遍扫描、并行 dir_size、macOS 3 线程、SafetyLevel 结果分区、Analyzer 增量树 + 扫描中导航、TOML 规则。

**第一手体验差距（编排者亲测，direct 证据）:**
1. **Analyzer 标记删除(`d`)是死路**——标记后 `marked_for_delete` 无任何 consumer 执行删除；对标记目录按 Enter 只会钻进去。一个"清理工具"的分析器竟不能清理。
2. Analyze **扫描中列表不排序**（乱序：.net 284MB 排在 diamond 50kB 之下），只有 finalize 后才排；dua 扫描中即有序且自动跟随最大项。
3. **退出模型混乱**：实时扫描按 Esc 不回菜单，而落入 `[部分扫描]` 浏览器；q/Esc/Backspace/h 在各状态语义不一致。
4. **三堆叠边框盒子**（面包屑/目录信息/列表）浪费竖向空间；Menu 也大片留白。dua 只用单行 header + 单行 footer。
5. Clean 结果：计数单位混淆（"已选: 4 个" vs "201780 个文件"，项目/文件混用）；选中信息在顶栏和底部盒子**重复**；确认框只显示数量+大小，**看不到将删哪些路径**；模态打开时底部 hint 不切换上下文。
6. **Uninstall 无搜索/过滤**（尽管 CLI 已有 `--search`），61 个应用平铺，无残留文件预览。
7. 全局无 `?` help 覆盖层、无滚动条、无翻页/Home/End/Ctrl+d/u、无 glob 过滤、无鼠标；进度条粗（solid █/░）；无 NO_COLOR/浅色/单色主题。

**dua-cli 成熟基线（对齐目标，源码在 `/Users/zhaohejie/workspace/explore/dua-cli`）:**
- 单行 header(路径+可见/总数+大小+内联键位) + 单行 footer(排序模式+总量+**吞吐率 entries/s**+实时扫描指示)。
- `(press ? for help)` + 分类可滚动帮助覆盖层（键位的**单一事实来源**）。
- 右侧滚动条；多 pane（主视图 + **Marked Items pane**，Tab 循环焦点，`FocusedPane` enum + `cycle_focus`）。
- **标记→删除闭环**：`x`/`space`/`d` 标记进 Marked pane，`Ctrl+t` 移废纸篓 / `Ctrl+r` 永久删（help 标注"不可撤销"）。
- 导航 j/k、Ctrl+d/u(±10)、PageUp/Down、H/Home、G/End、o/l/Enter 进、u/h/Left 退；排序 s/m/n/c(可切升降)；列切换 M/C；`Shift+o` 在关联应用打开；`/` git-glob 过滤(Ctrl+f 切大小写)；r/R 刷新；小数块进度条(亚格精度)。
- ratatui 惯用法：`Scrollbar` 是独立 `StatefulWidget`（**必须设 `content_length` 否则渲染空白**）；居中弹窗 = `Rect + Clear`；搜索输入建议用 `tui-textarea` crate。
- 空白点（可超越基线）：dua/gdu 均**未**实现 `NO_COLOR`；鼠标是 opt-in。

## Topic Axes

1. **信息架构与空间效率** — header/footer、边框盒子、滚动条、列
2. **导航与列表操作** — 翻页/跳转、排序、glob 过滤、刷新
3. **标记→删除闭环** — Marked-items pane、从 Analyzer 执行删除、确认安全性
4. **反馈与可发现性** — help 覆盖层、键位提示、实时扫描反馈、状态行
5. **交互一致性与退出模型** — Esc/q/Backspace 语义、状态转换、模态上下文

> 48 个原始候选（6 帧 × ~8），去重后按"是否直接闭合亲测差距 × 杠杆/复利 × dua 对齐 × 架构可行性"排名，保留 7 个幸存者。

---

## Ranked Ideas

### 1. Analyzer 标记→删除闭环（打通死路）
**Description:** 让 Analyzer 的 `d` 标记有出口——复用现成的 `Confirming → Cleaning → Done` 管线（`Engine::clean` + 废纸篓）执行删除。加一个"已标记"审阅视图（可逐项撤销、显示待删总量），`Ctrl+t` 移废纸篓 / `Ctrl+r` 永久删。把只读浏览器变成真正的清理入口。**前置**：先统一"标记"机制（见下方 downside），让 Analyze 与 Results 共用同一套按路径寻址的 selection 存储。
**Axis:** 3 标记→删除闭环
**Basis:** `direct:` lib.rs:1046/1112 `d` 只做 `marked_for_delete.insert/remove`，全仓 grep 无 consumer 执行删除；`handle_analyzer_key`(1006-1057) 无执行删除路径，对比 `handle_confirm_key`(942) 才真正调 `Engine::clean`。编排者亲测确认死路。`external:` dua Marked pane + Ctrl+t/Ctrl+r 闭环。
**Rationale:** 6 帧全部独立指向此项——最强共识。这是最伤主体身份的缺陷：清理工具的分析器不能清理，用户找到大目录后只能记下路径去 Finder 手删。清理管线已完备，本质是"接线"而非造轮子。
**Downsides:** 需先收敛两套标记机制（`ScanItem.selected` vs `marked_for_delete`）为一套，否则闭环会加深割裂。Analyzer 删的是任意目录（非规则命中项），确认框必须显示真实路径 + 强化风险提示（见 #6）。
**Confidence:** 95% | **Complexity:** Medium | **Status:** Unexplored

### 2. 统一 Keymap 分发表 → 自动驱动 help + hint + 一致退出语义
**Description:** 建一张集中的 `Action` enum + 键位声明表（每状态声明其支持的 Action 子集）。主循环查表分发键位；**同一张表**自动生成 `?` help 覆盖层内容和底部动态 hint 行——键位成为单一事实来源。顺带确立全局退出契约：`Backspace/h/←`=上一级，`Esc`=取消/返回，`q`=退出应用，并让实时扫描 Esc 干净地回菜单。
**Axis:** 5 交互一致性与退出模型（跨 4 可发现性）
**Basis:** `direct:` lib.rs 6 个 `handle_*_key`(437-1123) 各自重写 `Up|Char('k')` 等；退出语义不一致（Results Esc→menu 见 :955，AnalyzingLive Esc 非空弹级/空则落入 partial 见 :1101-1108，静态 Analyzing Esc=pop 而 q=menu 见 :1038）；全局无 `Char('?')`；hint 是各页硬编码静态串且与实际支持键不完全一致。`external:` dua eventloop.rs 按 focused pane 集中分发 + `?` 帮助即键位单一事实来源；居中弹窗 = Rect+Clear（confirm.rs:17 已有此惯用法可复用）。
**Rationale:** 最高杠杆的一次性投入。建成后每加一个键位或页面，help 与 hint 自动同步、退出语义天然一致——把"可发现性基础设施"从零变成免费副产品。#5、#3 都建其上。直接解决亲测差距 3(退出混乱)、7(无 help)。
**Downsides:** 需重构现有 6 个 handler 到声明式表，初期改动面大；表的抽象粒度（全局 vs per-pane）要设计好。
**Confidence:** 85% | **Complexity:** Medium-High | **Status:** Unexplored

### 3. 共享列表组件 StatefulList：滚动条 + 翻页/跳转 + `/` 过滤（含 Uninstall 搜索）
**Description:** 抽象一个带 cursor + scroll offset + marked + 分隔行跳过的列表状态组件，内建 ratatui `Scrollbar`（设 `content_length`）、翻页（Ctrl+d/u ±10、PageUp/Down、H/Home、G/End）、以及 `/` 唤起的增量过滤（glob 或 fuzzy，用 `tui-textarea` 承载输入）。Results 与 Analyzer 两套手写列表状态收敛成一个。Uninstall 首个受益——61 个应用可即时搜索而非纯滚动。
**Axis:** 2 导航与列表操作
**Basis:** `direct:` results 用 `result_cursor/result_scroll` + 手写 `skip_separator_forward/backward`(app.rs:203-221)，analyzer 另用 `cursor/cursor_stack/nav_path`(app.rs:36-44)，两套光标逻辑不共享；全 ui 层无 `Scrollbar`、无 PageUp/Home/End；`App` 无任何 filter/query 字段。编排者亲测"Uninstall 无搜索、61 应用平铺"。`external:` dua Ctrl+d/u、H/G、`/` glob(Ctrl+f 切大小写)、右侧滚动条；ratatui Scrollbar 须设 content_length；tui-textarea 承载搜索输入。
**Rationale:** 闭合最持续的累积摩擦——清理场景天然面对长列表（几百缓存项/应用），只能单步 j/k 意味着找目标按几十下。一次抽象让所有列表页同时获得滚动条/翻页/过滤，并为 #1 的 Marked pane 复用同一容器。
**Downsides:** 过滤用 glob 还是 fuzzy 需定；引入 tui-textarea 是新依赖（评估 vs 手搓输入）；过滤逻辑复用 core `--search` 还是 TUI 层另起需跨 crate 对齐。
**Confidence:** 85% | **Complexity:** Medium | **Status:** Unexplored

### 4. 扫描中实时按 size 排序 + 自动跟随最大项
**Description:** `AnalyzingLive` 增量构建时当前层 children 按 size 降序显示（渲染前对当前层快照排序，或节流重排），并像 dua 一样光标自动跟随最大项——除非 `user_navigated` 已置位。消除"扫描中乱序/跳序"死角。配套可考虑移除独立的 `Sorting` 转场态。
**Axis:** 4 反馈与可发现性（跨 2 列表操作）
**Basis:** `direct:` `finalize()` 的递归排序只在 `transition_to_sorting` 后台线程发生(lib.rs:142-152/871-875)，`AnalyzingLive` 阶段 `integrate_entry` 只 push(lib.rs:83-88)，children 是 jwalk DFS 序未排；`user_navigated` 标志(1082) 已可用于"仅未手动时跟随"。编排者亲测乱序。`external:` dua 扫描进行中即按 size 排序 + 跟随最大项。
**Rationale:** 扫描是用户盯着看的高峰时刻，乱序等于这段时间信息无效，把"实时"卖点浪费掉。是 dua"成熟感"最直观的一点。
**Downsides:** 每条 entry 重排 vs 节流重排的性能权衡，需与已优化的扫描管线协调（只排当前可见层可控成本）。
**Confidence:** 80% | **Complexity:** Medium | **Status:** Unexplored

### 5. 共享 Chrome 组件层：单行 header/footer 取代三堆叠边框盒 + 统一 spinner tick
**Description:** 把每个 ui/*.rs 重复的边框盒、标题头、底部 hint、spinner 抽成可复用组件（`render_frame(header, body, footer)`）。各页从"自绘整套 layout"缩成"填 body"。Analyzer 三堆叠盒 → 单行 header(路径+统计+内联键位) + body + 单行 footer(排序模式+总量+吞吐率)。spinner 的 tick 由 throttle 循环维护单一 `tick: u64` 存入 AppState，UI 只读不各自算 `SystemTime`。
**Axis:** 1 信息架构与空间效率
**Basis:** `direct:` `Borders::ALL` 在 ui 层硬编码 ~18 次；`SPINNER_FRAMES`/`spinner_char` 在 scan.rs:10、analyzer.rs:13 重复定义，`SystemTime::now()` 算 tick 出现 4 处(scan.rs:119/312、analyzer.rs:277/366)；menu.rs:16-23 `Length(5)+Min(10)+Length(3)` 致 4 项菜单大片留白。编排者亲测三堆叠盒浪费空间。`external:` dua 单行 header+footer 基线。
**Rationale:** 一处改风格全状态生效，立刻回收 2-4 行竖向空间给列表；消除 spinner 各页 tick 漂移。同时是 core/UI 分层第一块砖——为"多界面适配"轨道的 GUI 复用铺路（复利）。
**Downsides:** 是全局视觉语言的统一决策（是否所有页转 header/footer 范式），影响面广，需先定组件契约。
**Confidence:** 85% | **Complexity:** Medium | **Status:** Unexplored

### 6. 确认安全性升级：待删路径清单 + 可逆性梯度 + 计数语义统一
**Description:** 三合一的"删除前防线"升级：(a) 确认框从"数量+大小"升级为**可滚动的待删路径清单**，可逐项剔除；(b) 显式表达**可逆性梯度**——移废纸篓(可还原) vs 永久删(不可逆，强确认/措辞警告)，用色阶/图标区分；(c) 统一"item/file/category"术语，选中信息只在一处呈现，消除"项目 vs 文件"单位混淆与顶/底重复。
**Axis:** 3 标记→删除闭环（跨 1 信息架构）
**Basis:** `direct:` confirm.rs:19-62 仅渲染 `selected_summary()` 的 count/size + "文件将移至废纸篓"，无路径列表；results.rs:47/53/190 同页三处计数单位混用且顶底重复；lib.rs:942 confirm Enter/y 立即执行。编排者亲测确认。`external:` dua 区分 Ctrl+t(废纸篓)/Ctrl+r(永久，help 标"不可撤销")；git 式可逆性分层 / email 垃圾箱可恢复的心智模型。
**Rationale:** 删除是清理工具最高危动作，"盲删"确认框违背最基本安全承诺；计数歧义会让用户误判删除规模，是信任问题。与 #1 共用同一确认组件。
**Downsides:** 术语统一牵涉 core 层词汇（跨 crate）；"确认要展示多少细节才够安全"是产品底线判断，需团队定。
**Confidence:** 85% | **Complexity:** Low-Medium | **Status:** Unexplored

### 7. 超越基线：NO_COLOR + 形状编码 SafetyLevel + 自适应小终端布局
**Description:** 两个把 mc 推到 dua/gdu 之上的差异化点：(a) 支持 `NO_COLOR` 环境变量与单色/浅色主题，且 SafetyLevel 不仅靠颜色，用**形状/符号**冗余区分（如 ● Safe / ▲ Moderate / ✕ Risky），保证色盲/浅色/管道环境信息不丢；(b) 布局按实时 `frame.area()` 分档降级——极小终端(如 40×10)折叠边框盒、压缩 hint 为图标，避免破版。
**Axis:** 4 反馈与可发现性（跨 1 空间效率）
**Basis:** `direct:` scan.rs:18-20/199-201 安全级硬编码 `Safe→Green/Moderate→Yellow/Risky→Red`，无 NO_COLOR 检测、无形状；menu.rs:16-21、scan.rs:40-64 全用固定 `Length/Min`，`Min(10)` 在 10 行终端挤爆。`external:` NO_COLOR 是既定规范(no-color.org)，dua/gdu 均未实现——超越基线的机会；gdu `-c` 单色模式先例。
**Rationale:** 安全级是清理工具核心信息，仅靠颜色对色盲/浅色/管道用户等于信息缺失（安全后果）；responsive 让工具在任意终端"可用"而非"破版"。做到即领先现有开源竞品。
**Downsides:** 主题系统是新增基础设施；形状编码要在窄行里与现有列排版协调。属"raise the bar"而非闭合基线差距，优先级低于 #1-#6。
**Confidence:** 75% | **Complexity:** Medium | **Status:** Unexplored

---

## Rejection Summary

| # | Idea | 帧 | Reason Rejected |
|---|------|----|-----|
| 1 | 鼠标支持(opt-in --mouse) | 约束#7 | dua/gdu 均设为 opt-in 低优先级；键盘成熟度应先行；事件管线改动换取边际成熟度收益，P2 |
| 2 | 撤销倒计时 toast(Gmail undo) | 类比#2 | 文件已入废纸篓天然可恢复；在删除闭环(#1)存在前是镀金；"可逆性梯度"已并入 #6 |
| 3 | checklist 式确认仪式(航空/手术) | 类比#6 | 对当前场景过度设计；"强确认与后果成正比"已并入 #6 |
| 4 | 常驻双栏/Miller columns | 类比#4 | 与 #1 的 Marked 审阅视图重叠；完整双栏是 P2 布局工程 |
| 5 | 可编辑播放队列式 Marked pane | 类比#8 | "逐项撤销/跳回源"已并入 #1 的 Marked 审阅视图 |
| 6 | 超宽多列信息布局(300 列) | 约束#2 | 多数终端非超宽；responsive-小终端(#7)收益更实在 |
| 7 | 自动预选安全项 + 智能默认 | 反转#7 | 已部分存在(`select_all_safe` + Clean 自动选 Safe)；"确认路径可见"已并入 #6 |
| 8 | Menu 入口范式重构(直接进浏览器+切镜头) | 假设#4 | 属下方"统一管线"大重构；作为战略方向而非快赢 |
| 9 | 全局 tick 时钟(独立提案) | 反转#2 | 影响小，已并入 #5 共享 Chrome |
| 10 | FocusedPane 焦点模型(独立提案) | 杠杆#7 | 是 #1 Marked pane 与 #3 过滤输入的实现手段，非独立价值项 |

## Open Product Question — 是否统一四命令为单一管线？

**问题（假设打破帧的最深候选，跨越"快赢"范畴）:** Clean / Uninstall / Analyze / Purge 目前是**四套割裂的独立流程**，各有状态机与交互心智模型（Analyze=树浏览+标记；Clean/Purge=分类勾选+确认；Uninstall=应用平铺）。dua 的"成熟感"很大程度来自其**单一贯穿的心智模型**（树浏览→标记→Marked pane→删除）。

**方向:** 把四命令上抬为"同一浏览器 + 不同扫描源/镜头"的单一管线——用户学一次导航/标记/执行，四处通用；命令差异退化为"喂给管线的数据源不同"。这与 #1(删除闭环)、#2(统一 keymap)、#6(统一确认)天然收敛到同一套交互。

**为何单列:** 这是架构级战略赌注（大 scope、需推倒重来 `back_to_menu` 的清空逻辑与四套 UI），不是本轮 ideation 定义的"下一步快赢"。**建议用 `/ce-brainstorm` 单独探讨**：本轮的 #1/#2/#3/#5/#6 恰好是通往这个统一模型的增量基石——即便最终不做完全统一，这些幸存者也各自成立。

## 建议实施顺序（含依赖）

```
地基:   #5 共享 Chrome + StatefulList(#3 载体)   →   #2 统一 Keymap 表(驱动 help/hint/退出)
        （统一"标记"机制作为 #1 前置，在此阶段一并收敛）
闭环+对齐: #1 Analyzer 删除闭环   +   #4 扫描中实时排序   +   #6 确认安全性升级
操作力+超越: #3 导航/滚动条/过滤(Uninstall 搜索)   →   #7 NO_COLOR/形状/responsive
```

- **#1 是共识第一（6 帧全指向），也是最伤定位的缺陷**——若只做一件事，做它（但先收敛标记机制）。
- **#2、#5 是复利地基**：先做它们，后续每项改进都更便宜、且天然一致；也为 GUI 轨道铺路。
- **#4 是 dua 对齐最直观的一击**，改动集中、见效快。
