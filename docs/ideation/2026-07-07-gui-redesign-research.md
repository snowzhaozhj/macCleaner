---
date: 2026-07-07
topic: gui-redesign-research
focus: GUI 界面与交互重设计的调研 grounding——竞品形态谱系 + 防跳变/可视化 + 双用户调和/信任交互
mode: repo-grounded + web-research
grounding: 通读 GUI 前端全量代码（Tauri+Svelte）+ DESIGN.md/PRODUCT.md/next-step-tui-vs-gui + 3 个并行子代理 Web 调研（真调用 WebFetch，本环境内置 WebSearch 被网关禁用）
status: 研究记录（ce-ideate Phase 1 grounding）——后续接排序化 ideation 与 ce-plan
---

# GUI 重设计调研记录

> 用户诉求（原话）："重新整体设计和优化 GUI 界面和交互，现在整个页面非常难看，而且跳变严重，频繁上下跳动，用户体验不好，完全像是把 TUI 搬过去了，这两种用户的诉求是不一样的。"
>
> 本文是重设计前的**调研 grounding**，不含排序化候选方向（那是下一步 ideation 的产物）。目的：把"现状病根 + 业界怎么做"钉成事实基线，供 ideation 与 plan 引用。

## 1. 现状诊断（源码事实，file:line）

技术栈：Tauri（Rust 后端 `crates/gui/src`）+ Svelte 5 前端（`crates/gui/frontend/src`，用 runes `$state`/`$derived`）。屏：`App.svelte`（外壳+tab）、`routes/Clean.svelte`、`routes/Analyze.svelte`、`routes/Onboarding.svelte`；组件 `lib/Safety.svelte`、`lib/ConfirmDelete.svelte`；`lib/tokens.css` 是唯一语义 token 源。

### 1.1 "跳变/上下跳动"的三个根因

1. **每个 phase 重排整个 body（最大元凶）。** `Clean.svelte` 布局随 phase 增删区块：`idle` 是 `margin:auto` 垂直居中 hero（`clean.svelte:274`）→ `scanning` 顶部插入 statusbar（`:164`）→ 切 `results` 时 statusbar **消失**、actionbar+skipped **出现**（`:210-241`）。区块来去 → 主体每次相位切换整体重排。`Analyze.svelte` 同构。外壳本身是稳定三区（`App.svelte:74`），但 route 内部把它推翻了。
2. **流式数据涌进无约束的列表。** 扫描期结果实时渲染（`Clean.svelte:176`），每个 `Found` 事件都重算 `groups`（`:34`）、新行/新分区从底部挤出、`.results` 的 flex 高度又随上下兄弟增删变化（`:330`）。无骨架占位预留空间，行是"长出来"的。
3. **终端残留物。** `⠋` braille spinner（`:165`，纯终端产物）、`direction: rtl` 路径截断 hack（`:322`，文字重新锚定）、`▶` 字符当按钮（`Analyze.svelte:249`）。

### 1.2 "难看"的根因：全等宽文本网格

DESIGN.md 只要求"数据列等宽"，实现却让**一切**都是终端观感——扁平文本行、发丝边框、无尺寸可视化（Clean 连体积条都没有，只有 Analyze 有）、安全等级被压成小 mono 字形。既无 Linear/Raycast 的层次，也无消费级工具的"一眼看懂"。

### 1.3 "像把 TUI 搬过去"的定位病根

不是实现马虎——`DESIGN.md §9` 本身就叫《桌面移植检查清单》，把 GUI 定义成 TUI 的像素级移植（"按此表逐项落地即可保持与 TUI 同源识别"）。病根在设计文档的定位，不在 CSS。

### 1.4 战略重构：GUI 到底给谁

`docs/ideation/2026-07-07-next-step-tui-vs-gui.md` 已论证：**主要用户（开发者）已被 CLI+TUI 充分服务**；GUI 存在的唯一理由（beat-mole #1 / STRATEGY）是**触达 CLI/TUI 到不了的二次用户——普通 Mac 用户**（也就是 CleanMyMac/Mole App 服务的那群人，我们把同类能力做得免费/开源/诚实/轻）。把 TUI 搬进 GUI 两头不讨好：对新手太生，对老手不比 TUI 强。**GUI 应普通用户优先，同时用渐进披露保住开发者可用。**

## 2. 竞品形态谱系（Web 调研）

清理/磁盘类软件分四派：

1. **磁盘可视化派**（DaisyDisk / GrandPerspective / OmniDiskSweeper / Sensei）：核心屏是"什么占空间"的一张图——DaisyDisk 放射 sunburst 圆环、GrandPerspective treemap 矩形树、OmniDiskSweeper Finder 多列列表。哲学"**只展示、你决定删**"，几乎不预选、不判安全（DaisyDisk 仅靠 safety stoppers 保护系统文件，且是永久删除）。适合**开发者/进阶探索式导航**。
2. **一键清理派**（CleanMyMac / MacKeeper）：核心屏一个大按钮（Smart Care 五合一），帮你决定一切，用**威胁计数/杀毒叙事**（"329K threats removed"）制造焦虑建立依赖。**我们反对的恐吓营销原型**。
3. **开发者/命令行派**（Mole / DevCleaner / ncdu·dua·dust）：分类列表 + 键盘导航 + **dry-run 预览 + 移废纸篓可撤销**。DevCleaner/Mole 都把"**先审阅、只移废纸篓**"当第一卖点。**Mole 是直接对手**：CLI 免费开源，GUI（mole.fit，付费/终身更新）加了 disk maps、visual cleanup review、menu bar HUD——把 TUI 能力图形化正是它的护城河，也是我们要正面争的点。
4. **通用桌面形态参考**：Raycast=键盘优先命令面板 + 图标卡片行；Linear=暗色极简 + Cmd+K + 密集列表 + 单一强调色；macOS 储存空间=顶部分段横条 + "Recommendations"建议措辞（冷静、无告警色）。

## 3. 防跳变与可视化（Web 调研）

### 3.1 关键权衡：可视化的位置

- **treemap** 对新手是"啊哈"屏（面积=体积，直觉强，GrandPerspective 式实现简单）；**但 treemap 为优化块方正会牺牲位置稳定性**（来源 Wikipedia Treemapping）——流式数据下块会乱跳，**直接撞我们的核心痛点**。sunburst 外圈小扇区还难点选。
- 结论：**可视化只做扫描后的静态总览（计算一次），live 面永远是列表 + 内联体积条**。一维列表重排远比二维块重排易驯服。开发者场景朴素列表+排序+多选本就优于图形块。

### 3.2 防跳变落地清单（最有效 → 次要）

1. **rAF/节流批处理流式事件**（对应：流式挤出 + 实时重排）——后台高频 emit 入缓冲，`requestAnimationFrame` 每帧只 flush 一次。抖动总闸门。[MDN rAF]
2. **列表重排用 FLIP / `animate:flip`**（对应：实时重排）——Svelte `svelte/animate` 的 `flip` 直接用于 keyed `{#each}`，按体积重排从"逐帧跳"变"平滑滑动"；合成层 transform 动画不触发重排、不计入 CLS。[aerotwist FLIP / Svelte docs / web.dev CLS]
3. **骨架屏 + 固定尺寸容器预留空间**（对应：相位切换重排 + 流式挤出）——扫描态直接渲染骨架行占位，别用"内容中间转圈"；分区/进度条/操作条用**固定高度容器**，出现时填内容而非撑开布局。[web.dev CLS / NN/G]
4. **`tabular-nums` 固定数字宽度 + 固定列宽**（对应：数字抖动）——体积/计数用 `font-variant-numeric: tabular-nums`，值跳动不改行宽。零成本。[MDN]
5. **View Transition API 处理相位切换**（对应：相位切换重排）——`startViewTransition` 包裹 Scanning→Results、分区增删，浏览器自动快照新旧态平滑过渡。[MDN View Transitions]
6. **CSS `contain` / `content-visibility` 隔离重排**——列表行/分区加 `contain: layout paint` 把一处更新的重排限制在局部；长列表 `content-visibility: auto` + `contain-intrinsic-size` 占位。[web.dev]
7. **虚拟滚动 + 固定行高**——数据量大时才需要，只渲染可视窗口。[web.dev]
8. **进度指示分级**（NN/G：>1s 才反馈；spinner 只配 2–10s）——扫描这种长/不定时长且有计数，应走**确定性百分比或"已发现 N 项/X 大小"**，而非无尽转圈。[NN/G]

> 优先级：1+2 直接消灭"每批重排"根因；3+4 消灭相位/数字层面位移；5–8 分场景增强。
> 注记：DaisyDisk "扫描时地图逐步生长"无权威来源证实（其官方材料只述"径向环+快"），不可臆断照抄。

## 4. 双用户调和 + 信任/安全交互（Web 调研）

### 4.1 双用户调和：推荐"智能默认 + 渐进披露单界面"，反对"Simple/Advanced 模式切换"

- 用 **NN/g 渐进披露**（同一界面：先呈现最重要项，专家主动展开次级项），而非两套 UI 或模式开关。贴合定位：一套界面里普通用户看到智能默认结果，开发者靠展开/快捷键拿密度与逐项审查。
- 三条硬约束（NN/g）：① 初级 vs 次级功能切分要准（靠使用统计）；② "如何进入下一层"显而易见（清晰标签 + 强信息线索）；③ **绝不超过两层**（更深层用户会迷路），别给次级项多条入口。
- 洞察：**TUI（开发者）与 GUI（普通用户）可以分形态，但同一形态内不要再切模式。**

### 4.2 信任/安全交互落地清单

- **type-to-confirm**：一手依据 MailChimp 输 "DELETE"（NN/g）、GitHub 删仓库。GUI 形态：Risky 项默认不勾选，删除时弹输入框要求键入 `delete`，Enter 不代替确认（与现核心一致）。仅对不可逆/高危用此门。
- **undo/废纸篓呈现**：NN/g 与 Apple HIG 都说"优先 undo 而非确认"。把"移废纸篓=可恢复"物理化为 undo——文案用"已移到废纸篓，可恢复"而非"已删除"；只有永久删除才走确认门。
- **确认要具体 + 描述性按钮**：不问"你确定吗"，说清删什么/影响/恢复；按钮用"删除 N 项 / 保留"而非 Yes/No；不默认选中"是"。
- **红色克制**（Apple HIG：`.destructive` 角色才自动变红）：红只留给 Risky 与永久删除；Safe/Moderate 不用红，避免"红色轰炸"。三通道编码（色+形+文字）正确，关键是别让红蔓延到安全项。

### 4.3 "避免变成 CleanMyMac"的正面设计原则

1. **永不伪造问题制造需求**（MacKeeper 反面：干净系统上报"严重"问题，诉讼认定"未做可信诊断"）——只展示真实可删项与真实体积，绝不夸大数字或编造告警。
2. **零暗黑模式且可见承诺**（对照 deceptive.design：假紧迫、confirmshaming、强制注册、难取消、隐藏订阅）——无倒计时、无羞辱式文案、扫描不需账号；把"开源/零遥测/零订阅/删即入废纸篓"做成 UI 里显式可见的信任信号。
3. **预选须透明且只勾安全可逆项**——预选本身是暗黑模式候选（deceptive.design: Preselection）。界限=默认只勾 `Safe 且 preselect`、永不勾 Risky、且明示为何勾选。
4. **克制优于打断**（Apple HIG）——弹窗滥用会被无视（狼来了）；用 undo/废纸篓承接日常动作，确认门只留给真正不可逆操作。
5. **可审查性即信任**——每项显示 impact/recovery 证据文案，开发者可逐项核查；透明的"为什么安全/危险"比营销话术更可信。

## 5. 综合：给 GUI 重设计的关键结论/约束

供 ideation 与 plan 直接引用的硬结论：

- **C1 定位**：GUI 普通用户优先 + 渐进披露保开发者；不做 Simple/Advanced 硬开关；不复制 TUI 的移植清单（作废 DESIGN.md §9 的"移植"定位，改为"共享语义、各自形态"）。
- **C2 防跳变是地基**：稳定三区外壳落到 route 内部（固定槽位，内容替换而非区块增删）+ rAF 批处理 + FLIP 重排 + 骨架 + tabular-nums。这是重设计的第一优先级，也是专业感/信任感的物理基础。
- **C3 可视化定位**：live 面用列表+内联条（可控、不跳）；treemap/图仅作扫描后静态总览（新手"啊哈"屏），非流式面。
- **C4 信任即武器**：把 Trash 默认 + dry-run + Risky 永不预选 + 证据标签（"缓存·会自动重建"/"日志·不可再生"）显性化——**用"证据"替"威胁"、诚实优于恐吓**，让用户放心删除；红色稀缺化。这是执行质量上的领先（做得更诚实），不是为与竞品对立而对立。
- **C5 开发者可达性 + 能力补全**：借 Raycast/Linear 的 Cmd+K + 暗色密集列表服务开发者的速度与逐项审查，并把 clean/purge/analyze/uninstall 四种能力都做进 GUI（现只有 clean/analyze）——**先服务好自己的用户**，把同类工具的能力做得免费、完整、顺手，不背叛工程气质。
- **C6 引擎不动**：复用 `mc-core` Engine facade + ProgressReporter，重设计只在呈现层；安全语义（SafetyLevel 三通道、preselect 解耦、type-to-confirm、默认 Trash）完整继承。

## 6. 来源

Web（均直接 WebFetch 成功；本环境内置 WebSearch 被网关禁用，改用 MCP 搜索+权威页抓取）：

- web.dev — Optimize CLS: https://web.dev/articles/optimize-cls
- aerotwist — FLIP your animations: https://aerotwist.com/blog/flip-your-animations/
- MDN — View Transition API: https://developer.mozilla.org/en-US/docs/Web/API/View_Transition_API
- Svelte — svelte/animate: https://svelte.dev/docs/svelte/svelte-animate
- MDN — requestAnimationFrame: https://developer.mozilla.org/en-US/docs/Web/API/Window/requestAnimationFrame
- web.dev — content-visibility: https://web.dev/articles/content-visibility
- web.dev — Virtualize long lists: https://web.dev/articles/virtualize-long-lists-react-window
- MDN — font-variant-numeric: https://developer.mozilla.org/en-US/docs/Web/CSS/font-variant-numeric
- NN/G — Progress Indicators: https://www.nngroup.com/articles/progress-indicators/
- NN/G — Progressive Disclosure: https://www.nngroup.com/articles/progressive-disclosure/
- Wikipedia — Treemapping: https://en.wikipedia.org/wiki/Treemapping
- Apple Human Interface Guidelines（destructive/confirmation）: https://developer.apple.com/design/human-interface-guidelines/
- deceptive.design（暗黑模式类型）: https://www.deceptive.design/types
- DaisyDisk: https://daisydiskapp.com/ · GrandPerspective · OmniDiskSweeper · Mole(mole.fit) · DevCleaner · CleanMyMac/MacKeeper（竞品官网/评测）

> 未核实项（已标注）：DaisyDisk 扫描时地图增量生长的具体做法；Raycast/Linear/1Password 具体设计原理的一手出处（案例断言仅作参考）。原始证据档案（更细的 file:line 与逐条 URL）在 ce-ideate 本轮 scratch：`/tmp/compound-engineering/ce-ideate/a2d1cd01/evidence-*.md`（临时，会失效）。
