---
id: plan-008
title: "fix: TUI 体验第三轮——修复信任链最后一公里"
date: 2026-07-05
status: ready
origin: .impeccable/critique/2026-07-05T08-36-47Z__crates-tui.md
type: fix
depth: deep
---

# fix: TUI 体验第三轮——修复信任链最后一公里

> **Handoff 说明（给实施会话）**：本计划由 impeccable 双代理体检（Assessment A 设计评审 + Assessment B 确定性取证，均 tmux 实跑）+ Fable 综合设计产出，所有问题均有实测证据、所有决策已定稿——实施时**不需要重新调研**，按 KTD 逐条落地即可。工作分支 `feat/tui-ux-round3` 已建好（worktree：`../macCleaner.worktrees/tui-ux-round3`）。完整体检报告见 origin 快照（主仓 `.impeccable/critique/2026-07-05T08-36-47Z__crates-tui.md`）。

## 概述

基于 impeccable 体检（**29/40**，1×P0 + 2×P1）的针对性修复轮。前两轮成熟化（PR #11/#12）已把交互骨架拉到 dua 水位；本轮修的是**证据链最后一公里**：确认与完成两个信任峰值时刻的渲染质量配不上防错机制质量，外加一个 correctness 级的路径归属缺陷。

范围横跨 `core`（扫描路径归属）与 `tui`（确认框、chrome 韧性、终点屏、微对齐批次）。**不含**：结果页排序、过滤匹配分类名、Purge 根路径选择、Cleaning 可取消、预选策略重议（见"遗留 TODO"）。

## 问题背景（实测证据，双代理独立复现）

1. **[P0] 同一路径跨分类重复聚合**：`~/Library/Caches` 同时以"系统缓存 35.94GB"和"浏览器缓存 1.63GB"两个条目出现（同一 `PathBuf`）。勾选耦合（一个翻转另一个跟着变）、header"已选 4 项"vs toast"已标记 3 项"、确认清单同路径两行；用户以为删 1.63GB 浏览器缓存，实际 `marked` 的是整个 `~/Library/Caches`。
2. **[P1] 确认框在审查时刻失效**：路径尾部被右边框硬切（信息量在尾部：哪个项目的 node_modules）、Risky 后果句切半（`confirm.rs` Paragraph 无 wrap）、1228 项只可见迭代序前 8 项（预览率 0.65%，且非最大 8 项）；**80×24 下 type-to-confirm 的输入行与 `[Esc] 取消` 被整行裁掉**（`confirm.rs:136-139` 高度=行数+2 经 `centered_rect` 钳制后裁尾，恰好裁掉操作指引）。
3. **[P1] Chrome 文本韧性缺失**：120 列 footer 止于"a 全选安全"，`x 删除`/`? 帮助`不可见（新手不知道怎么删、不知道有帮助）；≤80 列 header 左右区零间隔粘连（`…site-packages/o已发现 1 项`，`chrome.rs:44-57` 硬裁剪无 gap 无省略号）；面包屑尾部硬截断丢当前最深层段（`analyzer.rs:379` 无省略逻辑，DESIGN.md §5.2 声明中段省略）；帮助浮层窄端截断（固定 48% 宽）。
4. **[P2] 终点薄弱 + 失败静默**：Done 屏一行字配 30 行空框；Results 路径删除失败信息被丢弃（analyzer 路径反而有"N 项失败，仍保留"）——违背"无静默"承诺；未复述"已入废纸篓可恢复/清空废纸篓才真正释放"（用户发现磁盘空间没变会认定工具骗人）。
5. **[P2] 视觉噪音与微对齐**：Analyze 首屏 29/30 行黄色（大文件高亮 + 体积降序 = 首屏全高亮，且颜色是唯一通道）；size 列 `{:>8}` 被 9 字符值（`406.73 MB`）溢出致 %/体积条整行漂移 1 格（`analyzer.rs:160`）；toast 无生命周期（分析完成后"扫描进行中不可标记"仍挂着）；过滤无匹配全空白无占位；目录称"1 个文件"；Done footer 两条提示同一动作；Scanning 态按 Space 静默（AnalyzingLive 同动作有 toast 解释）。

## 关键技术决策 + 实施细节

### KTD1 — 扫描流式上报按"匹配基路径"而非分类名归属（P0 根修）

**根因**（已代码级确认）：`scanner.rs` `flush_category_deltas`（:490-512）把所有分类的增量统一以 `root_path` 上报；子规则（浏览器缓存，真实模式 `Library/Caches/Google/Chrome`、`com.apple.Safari`、`Firefox`）的 TUI 条目因此顶着根路径 `~/Library/Caches`。

**实施**（`crates/core/src/scanner.rs`，改动集中在 `scan_with_rules` + flush 函数）：
- `cat_meta: HashMap<String, Meta>`（:214-225）→ `base_meta: HashMap<PathBuf, Meta>`：`root.path→root.meta` + 每个 `child_path→child meta`。顺带**删除**"同名分类 safety 唯一"的 `debug_assert`（按路径索引后该脆弱不变式不再需要）。
- 累加器改键（:227-231）：`size_by_category`/`emitted_by_category: HashMap<String,u64>` → `size_by_base`/`emitted_by_base: HashMap<PathBuf,u64>`。
- 最长前缀匹配处（:248-254）同时取出基路径：`map_or_else(|| (&root.path, &root.meta), |(p, m)| (p, m))`，文件 size 累加到 `size_by_base[base]`。
- `flush_category_deltas` → 重命名 `flush_base_deltas(reporter, size_by_base, emitted, base_meta)`（`root_path` 参数删除），emit `meta.found(base.clone(), delta)`。`Meta::fallback`（:87-95）随之无消费者，删除。"首次出现即使 0 也上报建项"的现有语义保持不变（`last==None && cum==0 → delta 0`）。
- **测试**：`scan_clean_streams_multiple_categories_under_one_root`（:791）的 doc 注释断言的正是 bug 行为（"两个分类都以 path=root.path emit"），按新语义改写；`SizeReporter`（:726）增记 `HashMap<String, HashSet<PathBuf>>`（分类→收到的 Found 路径集合），新增断言：`子缓存` 的 Found 路径 == `{sub}`、`根缓存` == `{root}`（这是 P0 的回归测试）。`scan_clean_streamed_found_deltas_sum_to_true_total` 应不受影响（单规则单根）。

**效果**：浏览器缓存条目获得真实子路径 → 独立可勾可删；勾选耦合、清单重复、计数失真全部消失。TUI 端 `(category, path)` 合并逻辑无需改动。

### KTD2 — 父项包含未勾选子项时，确认框显式披露

KTD1 之后仍残留一个物理事实：勾选"系统缓存 `~/Library/Caches`"整目录删除必然连带其中未勾选的浏览器缓存子路径（size 归属已扣除、物理包含无法扣除）。**不做**删除粒度改造（枚举顶层逐个删会递归产生同类问题），改为**诚实披露**。

**实施**（TUI 侧，确认清单构建处）：构建 `confirm_delete` 时，对每个待删路径 `p`，检查扫描结果中**未选中**条目的路径 `q` 是否满足 `q.starts_with(p) && q != p`；命中则在确认框追加警示行：`⚠ 系统缓存 包含 N 个未勾选的子项（如 浏览器缓存），将一并删除`。复用现有"过滤视图外仍删除"⚠ 警示的样式（`state.warning`）与堆叠位（Risky 块之下、注脚之上）。条目数量级为几十，O(n·m) 双循环可接受。

### KTD3 — 确认框重构为"固定头 + 可滚动清单 + 钉底操作区"

**布局**（`crates/tui/src/ui/confirm.rs` 重构）：
- 框宽 64%→**80%**；高度 = min(内容所需, 屏高-2)。内部 `Layout` 三段：`Length(头部)` 标题+数量/大小行；`Min(1)` 清单区（可滚动）；`Length(底部)` 警示行（KTD2/过滤外）+ 废纸篓注脚 + token 输入/按钮行。**操作指引行在任何终端尺寸下永不被裁**（这是 80×24 实测事故的直接对策）。
- **清单内容分两层**：Risky 逐条全量（保持 plan-007 KTD7/R9 契约：置顶、红、含 impact/recovery，impact/recovery 行开 `.wrap(Wrap { trim: false })`）；非 Risky 改为**分类汇总行**（`● 系统缓存 — 3 项, 36.8 GB`）+ **最大 Top-3 路径抽样**（按 size 降序，替换现 `MAX_SHOWN=8` 的迭代序取样）。设计依据：1228 项场景"逐条前 8"是假审查，分类汇总+最大项抽查才是人能核对的粒度。
- **滚动**：`App` 增 `confirm_scroll: usize`（打开/关闭确认框时归零）。键位：`↑↓`/`PgUp/PgDn` 滚动清单区；**无 Risky 时** `j/k` 也滚动；**有 Risky（type-to-confirm 激活）时 `j/k` 归 token 输入缓冲**、不参与滚动（避免吞字符）。keymap.rs 为确认框态补注册这些键（footer/help 自动同步）。
- **路径渲染**统一走新 helper `ellipsize_middle(path, max_width)`：按**显示宽度**（`unicode-width`，ratatui 既有传递依赖）中段省略、保头保尾、home 前缀缩写为 `~`。放 `ui/` 公共位置（chrome.rs 或新 `ui/text.rs`），KTD5 面包屑与 KTD9 详情面板路径行复用。

### KTD4 — footer 按优先级降级而非硬截断

**实施**（`crates/tui/src/keymap.rs` + `ui/chrome.rs` footer 渲染）：
- keymap 条目增 `priority: u8`（0 最高）。分配：0 = 删除 `x`、帮助 `?`、退出/返回 `Esc`/`q`；1 = 标记 `Space`、确认输入、过滤 `/`、全选 `a`；2 = 进入/展开 `Enter`/`Tab`；3 = 方向 `↑↓/jk`、翻页、`g/G`（可自行发现）。
- 渲染：先按显示宽度累计装配全量；溢出则按优先级**从低到高整条剔除**（绝不留半截文字）直至放下。保证任何宽度下 `x 删除` 与 `? 帮助` 可见。`?` help 浮层不受影响（仍是全量单一事实源）。

### KTD5 — chrome 文本韧性三处

- **header**（`chrome.rs:44-57`）：左区截断保证与右区 ≥1 空格间隔，截断处以 `…` 结尾（按显示宽度裁剪，防 CJK 半字）。
- **面包屑**（`analyzer.rs:379` `build_breadcrumb_spans`）：超宽时中段省略——保首段 + 末段（当前所在层**必须可见**），中间折叠为 ` … `，对齐 DESIGN.md §5.2。
- **帮助浮层**（`ui/mod.rs`）：宽度自适应 `min(最长行显示宽+4, 90% 屏宽)`；行内容超宽时按显示宽度截断补 `…`。

### KTD6 — Done 屏重构为清理报告

**实施**（`ui/` Done 渲染 + lib.rs `CleaningDone` 处理）：`CleaningDone` 事件已携带的失败信息不再丢弃（当前 Results 路径只取成功数）。Done 屏展示：
- 成功 N 项 / 失败 M 项；失败逐条列出 路径 + 原因（M 大时截断显示前若干条 + "还有 K 项失败"计数，M=0 不显示失败区）；
- 释放量 + 双注脚：`已移入废纸篓，可从废纸篓恢复` / `清空废纸篓后才真正释放磁盘空间`（TUI 无 Permanent 路径，恒显示）；
- 按分类小结（分类名 + 项数 + 大小，数据从删除清单派生，勿新增冗余存储）；
- footer 修复"Enter 返回菜单 | q 返回菜单"重复（合并为一条）。

### KTD7 — toast 生命周期：状态转换时主动清除

实测残留场景：AnalyzingLive→Sorting→Analyzing 转换后"扫描进行中不可标记/删除"仍覆盖 footer。**实施**：在状态转换函数统一清 `status_message`（保留现有"下次按键清除"路径）。**不引入**定时器过期（静态态无 tick，成本不值）。注意别误清"转换瞬间刚设置的提示"（如 pending_leave 的"再按一次 q"）——清除动作放在**进入新 AppState** 的转换点，而非按键处理处。

### KTD8 — 大文件高亮只作用于文件且补第二通道

`analyzer.rs` `LARGE_FILE_THRESHOLD`（100 MiB）高亮：确认现状是否命中目录——实测首屏 29/30 行黄的来源即此（体积降序下顶部几乎全 ≥100MiB）。**实施**：黄色高亮只标**文件**（目录天然大，保持 `accent` 目录色）；命中的大文件加 `⚠ ` 前缀（颜色之外的第二通道，无障碍红旗）。若现状已仅文件生效，则只补符号通道并复核首屏噪音来源后调整（必要时阈值改相对值——超过父目录占比的判据，作为备选不强制）。

### KTD9 — 微对齐批次（逐项 ≤30 分钟）

1. size 列 `{:>8}`→`{:>9}`（humansize DECIMAL 最长 9 字符，`analyzer.rs:160`）；
2. `truncate_name` 与所有截断按**显示宽度**（`unicode-width`）而非 char 数计，消除 CJK 潜在错位；
3. 目录条目"1 个文件"术语改"N 项"；header/toast/确认框计数统一用"项"；
4. 过滤无匹配时列表区显示占位行：`无匹配项（Esc 清除过滤）`（muted）；
5. Scanning 态按 Space 给 toast 提示（对齐 AnalyzingLive 的"扫描进行中不可标记"行为，反馈一致性）；
6. Results 详情面板顶部加一行 muted 完整路径（`ellipsize_middle`）——顺带消解同名条目无法区分的残余困惑。

## 实施阶段（依赖顺序）

| 阶段 | 内容 | 建议执行者 | 依赖 |
|---|---|---|---|
| 1 | KTD1（core P0 修 + 回归测试） | 主会话或高档模型子代理（安全关键，需评审） | 无 |
| 2 | KTD3 + KTD2（确认框重构，触 confirm.rs / app.rs / lib.rs / keymap.rs） | 子代理（opus），主会话评审 | KTD1（清单不再有重复路径） |
| 3 | KTD4–KTD9（chrome/终点/微批次，触 keymap.rs / chrome.rs / analyzer.rs / ui/mod.rs / lib.rs） | 子代理（opus/sonnet），主会话评审 | 与阶段 2 **顺序执行**（都触 lib.rs/keymap.rs，勿并行） |
| 4 | 集成验收（见下） | 主会话亲自 | 1–3 |

## 验收协议（阶段 4，逐条核销）

工具：项目 `verify-tui` skill（`.claude/skills/verify-tui/`，tmux 跑法）。**安全红线**：删除流只针对自建临时假目录（伪造 fakehome + 假 node_modules/缓存）；真实数据的确认框只观察后 Esc。

- [ ] 同一扫描中不存在两个同 `PathBuf` 的可勾选条目；勾选互不耦合；header/toast/确认框计数一致（复现路径：Clean 扫描，看 系统缓存 与 浏览器缓存）
- [ ] 确认框 120×35 与 80×24：操作指引行（token 输入/[Esc]）始终可见；路径中段省略不顶框；Risky impact/recovery 完整可读（wrap）；>8 项清单呈现"分类汇总+Top3"且可滚动
- [ ] 勾选父目录且存在未勾选子项时，确认框出现 ⚠ 披露行
- [ ] 120 列 footer：`x 删除` 与 `? 帮助` 可见；80 列 header 左右区有间隔且截断带 `…`；面包屑深层钻取后末段可见（中段 `…`）
- [ ] Done 屏：成功/失败明细（可构造只读目录制造失败项）+ 废纸篓双注脚 + 分类小结；footer 无重复提示
- [ ] Analyze 首屏黄色不再主导；大文件带 `⚠`；size 列 9 字符值无漂移（构造 >100MB 文件验证）
- [ ] toast：分析完成后不再残留"扫描进行中"提示
- [ ] `NO_COLOR=1` 走查：反显光标、●▲✕+标签可辨、新增的 ⚠/披露行不依赖颜色
- [ ] `cargo test` 全绿；`cargo clippy --all-targets` 零警告（**worktree 内 clippy hook 不生效，必须手动跑**）
- [ ] 收尾：`/impeccable critique crates/tui` 复测，分数应 >29（快照自动对比趋势）

## 遗留 TODO（本轮显式不做，防散焦）

- 结果页按大小排序（`s` 键）；过滤匹配分类名 — Alex 红旗，独立小 feature
- TUI 内 Purge 根路径选择（当前固定 `~`，CLI 已支持 `mc purge <path>`）
- Cleaning 态取消 + i/N 进度（大体积移废纸篓可达数分钟且锁死）
- 预选 Moderate 的信任额度讨论（critique Q3，产品决策，建议 /ce-brainstorm）
- "释放 X GB"vs 废纸篓语义的全局措辞审计（本轮仅 Done 屏注脚兜底，critique Q2）
- Error 态信息丰富化；Uninstall 深度走查（本轮未覆盖；其 61 应用默认全预选值得下轮审视）
- 帮助浮层内容超高滚动（次要观察 #8 的纵向部分）

## 沉淀提醒（实施完成后）

- `/ce-compound`：KTD1 的"流式聚合键必须是删除粒度的键（路径），不能是展示粒度的键（分类）"值得进 `docs/solutions/`；`ellipsize_middle`/footer 优先级降级模式可复用于未来 GUI。
- DESIGN.md 同步:§5.1 footer 补"优先级降级"约定、§6 组件表确认框行更新、符号表若新增 ⚠ 用法补充说明。
- CONCEPTS.md：如"匹配基路径（base path）"成为稳定词汇则收录。
