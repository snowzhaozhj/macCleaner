---
date: 2026-07-07
topic: gui-redesign-ideation
focus: GUI 界面与交互整体重设计——消解跳变/难看/"TUI 移植感"/双用户诉求未分化
mode: repo-grounded
---

# Ideation: GUI 界面与交互整体重设计

7 个排序化重设计 move（从 40 个原始候选经新上下文 basis 验证 + impeccable 设计评审筛出），综合为分层方向并给出推荐。grounding 与竞品调研见配套文档 [`2026-07-07-gui-redesign-research.md`](./2026-07-07-gui-redesign-research.md)。

> **一句话结论**：做**方向 B（普通用户优先主线）**，按 A→B 其余分段施工。**方向 C（磁盘空间总览 + 菜单栏 HUD）不是被否决，是推迟到独立一轮**——它碰 app 生命周期/常驻进程、超出本轮呈现层范围，且须满足 `STRATEGY.md` 的"轻量、不成为系统负担"（约束的是**怎么实现**：常驻要轻、无角标无通知无高频后台扫描，不是"做不做"）；且其"空间总览可视化"大半已在 move 2，真正新增的只有菜单栏 HUD 那半。病根不在 CSS，在 `DESIGN.md §9` 把 GUI 定义成 TUI 的像素级移植——重设计的本质是废掉这个定位，改为"共享语义、各自形态"。
>
> **纠错声明**：早期版本曾以"照搬 Mole 付费卖点、要和竞品不一样"为由把方向 C 判"出局"——这条推理作废。差异化在**执行**（免费/开源/零遥测/诚实/快/轻/安全），不在省略预期功能或与竞品对立；品类基本盘功能缺了是短板。原则见 [`docs/solutions/product-decisions/differentiation-on-execution-not-opposition.md`](../solutions/product-decisions/differentiation-on-execution-not-opposition.md)。

## Grounding Context（Codebase Context）

- **栈**：Tauri（Rust 后端 `crates/gui/src`）+ Svelte 5 前端（`crates/gui/frontend`，runes）。屏：`App.svelte`(外壳+tab) / `Clean.svelte` / `Analyze.svelte` / `Onboarding.svelte`；`lib/tokens.css` 唯一语义 token 源（已核实合格：OKLCH、安全轴/状态轴分离、含对比度注释——**扩展而非推翻**）。
- **跳变三根因**（源码核实）：① 相位切换整块重排（`Clean.svelte:274/164/210-241`，最大元凶）；② 流式涌入无约束列表、行"长出来"（`:34/:330`）；③ 终端残留物（`⠋` braille、`direction:rtl` 截断 hack、`▶` 字符按钮）。
- **难看根因**：全等宽文本网格（DESIGN.md 只要求"数据列等宽"，实现却让一切都 mono）；Clean 连体积条都没有。
- **定位**：CLI+TUI 已充分服务开发者；GUI 战略目的（beat-mole #1 / STRATEGY）是触达 **普通 Mac 用户**，正面对位 CleanMyMac/Mole App。→ 普通用户优先 + 渐进披露保开发者。
- **不可退让约束**：完整继承安全语义（Risky 永不预选、type-to-confirm、默认 Trash、三通道编码）；只改呈现层复用 mc-core；`STRATEGY.md:14` 明文"不成为新的系统负担"（轻量）；反 CleanMyMac 恐吓营销/红色轰炸/伪造问题/暗黑模式预选。

## Topic Axes

- **A. 信息架构与导航**——单流程/tabs/命令面板；clean·purge·analyze·uninstall 的组织；默认屏与渐进披露层级
- **B. 扫描态与防跳变**——骨架/rAF批处理/FLIP/固定槽位/进度分级/相位过渡
- **C. 视觉语言与数据可视化**——列表 vs 分段条；等宽的边界；层次；安全三通道 GUI 形态；体积编码
- **D. 信任与破坏性操作交互**——预选可视化、type-to-confirm、undo/废纸篓、红色克制、证据替威胁
- **E. 双用户调和**——smart defaults、渐进披露、快捷键/命令面板、专家逐项审查

## Ranked Ideas

### 1. 稳定基座：外壳三区下沉 route + 分类行"0 检测"持久化 + StreamingList 原语

**Description:** 把 `App.svelte` 已有的稳定三区（header/main/footer）原则**下沉到 route 内部**：每个屏固定 `摘要区 / 列表区 / 操作区` 三个恒在槽位，相位切换只替换槽位**内容**，DOM 区块永不 mount/unmount。列表在扫描**开始瞬间**就按编译期已知的分类预渲染成行（骨架体积条+"扫描中"计数），流式 `Found` 只在原位更新宽度/数字——**关键**：命中为 0 的分类**保留为"未检测到"行**（机场到达板惯例），或在扫描完成时做**一次** settle 收拢，绝不逐个消失。扫描期**行序锁死**（继承 TUI"扫描中禁止按位置标记"教训），体积排序只在完成时一次性 FLIP settle（~200ms）。把 rAF 批处理 + FLIP + tabular-nums + 骨架封装成一个 `StreamingList` 组件，Clean/Analyze/未来 purge 复用。

```
恒在三区（内容替换，区块不增删）        扫描开始即预印全部分类行
┌ 摘要区 ────────────────────────┐   系统缓存      ▓▓▓░░░  扫描中
│  <内容随相位换，高度恒定>        │   浏览器缓存    ▓░░░░░  扫描中
├ 列表区（overflow, 视口恒高）────┤   开发产物      ░░░░░░  未检测到 ← 保留,不消失
│  行只填数字/长条, 永不挤出/重排  │   日志          ▓▓▓▓░░  扫描中
├ 操作区 ────────────────────────┤   
│  <禁用/激活切换, 位置恒定>       │   完成时一次 settle: 空行收拢 + 体积排序 FLIP
└────────────────────────────────┘
```

**Axis:** B
**Basis:** `direct:` 跳变根因逐行核实——`Clean.svelte:274`(hero margin:auto)/`164`(statusbar 增)/`210-241`(actionbar+skipped 换)；`App.svelte` `.shell` 已是固定三区。`external:` web.dev CLS "固定尺寸容器内替换内容"、Svelte `svelte/animate` flip、机场到达板"槽位先于数据存在且不凭空消失"。
**Rationale:** 这是诊断点名的"最大元凶"，且纯结构性修复（不改视觉/交互），投入产出比最高——不做它，后面任何视觉重设计都会在同一套会跳的骨架上再跳一次。
**Downsides:** basis 验证器警示：类目静态已知 ≠ 该机器每类都有命中——若空槽位处理不当（扫完才撤下），跳变只是**被推迟**而非消灭，故"0 检测行保留 or 一次 settle"是硬要求，不是可选项。"与 TUI 共享 StreamingList"不成立（Rust/ratatui vs Svelte 运行时不同）——只共享**设计契约/算法思路**，复用限 Svelte 路由内。FLIP 不可用于扫描进行时的实时重排（平滑的重排仍是每秒在动）。
**Confidence:** 90%
**Complexity:** Medium

### 2. 首屏=一句话答案 + 分段横条，列表降级为渐进披露第二层

**Description:** 默认屏不再是逐条列表，而是回答普通用户唯一的问题"能安全腾出多少"：一句话 **"可安全释放 12.4 GB"**（数字是句子主语，非悬浮奖杯）+ 一条 macOS 储存空间式**分段横条**（按分类着色、静态计算、低饱和、发丝分隔、图例带精确数值）+ 一个主按钮。逐条列表降级为"查看明细/点击分段"展开的第二层。数字来自**当前扫描累加**（非缓存），扫描中随发现增长，完成即定格。

```
┌──────────────────────────────────────────────┐
│  可安全释放  12.4 GB                           │  ← 一句话, 28-36px weight, tabular-nums
│  ▓▓▓▓▓▓▓▓ 开发缓存 ▓▓▓▓ 浏览器 ▓▓ 系统 ░ 其他   │  ← 静态分段条, 低饱和
│  开发缓存 8.1G · 浏览器 3.0G · 系统 1.3G       │  ← 图例带精确值
│              [ 移入废纸篓 · 释放 12.4 GB ]      │  ← 按钮量 === 上方数字
└──────────────────────────────────────────────┘
```

**Axis:** A / E
**Basis:** `external:` macOS 系统设置·储存空间"分段横条 + Recommendations"冷静形态（无告警色）；`direct:` `groups` 派生数据已存在只差渲染，现 idle 态已是 hero 文案+按钮只差预估数字。
**Rationale:** 与"TUI 移植"最彻底的决裂——第一层为普通用户的**决策**而非开发者的**审查**设计；列表消失后跳变载体也少一半。与 CleanMyMac "329K threats" 形成镜像：我们的大数字是**真实体积**不是威胁计数。
**Downsides:** impeccable 硬护栏——① 数字必须**精确等于主按钮将删的量**（文案说"可安全释放"而 selected 含 Moderate，差 1GB 信任全毁）；② 无渐变/光晕/settle 后 count-up 缓动滚数（那是营销动作）；③ 数字靠 weight 强调不用 96px display 字，否则滑向被禁的 hero-metric 模板；④ Safe 分段不许出现红系。
**Confidence:** 85%
**Complexity:** Medium

### 3. Trash 即 Undo：Safe/Moderate 删除去弹窗，type-to-confirm 只留 Risky

**Description:** 移废纸篓本身已是可恢复的物理 undo，不该在其上再叠"你确定吗"。把 Safe/Moderate 批次的确认弹窗整体移除：点删除直接执行 + 底部单实例可撤销 toast（"已移到废纸篓 · 撤销"，6–8 秒可见，撤销真实回滚列表状态）。modal + type-to-confirm 的重武器**完整保留**，但只留给**含 Risky 项**的批次。

**Axis:** D
**Basis:** `direct:` `ConfirmDelete.svelte:20-21` 核实——现状非 Risky 批量也走同一模态（注释自曝"纯非 Risky 批量走模态但不强制 token"）。`external:` Apple HIG/NN-g "优先 undo 而非确认"、product register "modal 是懒惰"、狼来了效应。
**Rationale:** 把"克制优于打断"字面落地——唯一保留的中断点是 Risky 的 type-to-confirm，反而让它真正意味着"这次不一样"（现状对可恢复操作也弹窗，稀释了危险确认的严肃性）。成本最低、收益最直接的一条。
**Downsides:** 若字面理解为"对全部项含 Risky 都拆确认"则违反硬约束（Risky 仅可经 type-to-confirm 删除）——必须严格限定 Safe/Moderate。toast 与 survivor 4 的回执**别双重播报**（toast 报动作，回执是 Done 态内容本体）。
**Confidence:** 88%
**Complexity:** Low

### 4. 证据替威胁：常驻证据文案 + 清理回执（EvidenceCard 组件）

**Description:** 规则表本就含 `impact`/`recovery` 证据文案，但当前 `.row` 模板完全没渲染（`Clean.svelte:13-14` 有数据、`:195-197` 只显示 Safety+path+size）。把它做成**常驻**信息：折叠行旁一行**弱化的纯文字**证据短句（"缓存 · 会自动重建" / "日志 · 不可再生"，muted ink，最多一个前置圆点）。清理完成出一张**排版化收据**（删了哪些路径 · 共 X GB · 去向废纸篓 · 如何恢复 · 耗时），强化现有 done 屏（`:250-256`）。抽取为 `EvidenceCard` 组件，Clean 行/确认弹窗/未来 purge 复用。

**Axis:** D / C
**Basis:** `direct:` 数据已在未渲染（核实）+ `Clean.svelte:185-198` 与 `ConfirmDelete.svelte:47-55` 行渲染高度重复，抽取合理；规则名（`rules.rs` 已有 "Docker Desktop Data"→"Docker" 具名映射）现成。`external:` "可审查性即信任"；MacKeeper 诉讼实证"未做可信诊断"是品类信任赤字来源。
**Rationale:** 竞品用威胁计数制造焦虑，我们用证据密度制造信任——收据是"用证据替威胁"的最终形态，且竞品结构上无法抄袭（CleanMyMac 商业模式依赖模糊）。纯呈现层，数据全在。
**Downsides:** impeccable 两个 slop 陷阱——① **chip-itis**：证据在折叠行必须是弱化文字，**不是填色药丸**（每行一个填色 chip 是 AI slop 高频形态），填色形态最多留展开态；② **信任徽章不进决策现场**：开源/零遥测徽章常驻删除按钮旁反而读作 marketing chrome，决策现场只放"移废纸篓·可恢复"，开源/零遥测归 About/onboarding。收据禁 confetti、禁大绿勾 hero。
**Confidence:** 85%
**Complexity:** Medium

### 5. 安全等级是空间地理，不是行内标签——Risky 折叠隔离

**Description:** 让三级安全成为结果区的**分区地理**而非每行一枚小徽章：Safe 区在顶部、默认展开、主按钮直达；Moderate 区其下、需一次点击展开、文案强调"需手动重建"；Risky 区在最底、**默认折叠**成一条收起横条（"N 项含不可再生数据"），展开后每项自带 impact 证据且删除必经 type-to-confirm。红色在全 app 只跟随 Risky/不可逆语义流动。

```
▸ 安全 · 8 项 · 8.1G   [默认展开, 主按钮直达]      区背景中性,
    xcode-derived   ● 会自动重建        4.1G       安全色只落在小圆点/图标,
    npm-cache       ● 会自动重建        2.0G       不染分区背景
▸ 中等 · 3 项 · 3.0G   [一次点击展开]
▾ 危险 · 2 项 · 1.3G   含不可再生数据  [展开需 type-to-confirm]  ← 1px低饱和红描边(非侧边条)
```

**Axis:** C / D
**Basis:** `reasoned:` 空间隐喻（危崖/黄坡/绿地）与"危险"语义天然一致；行级徽章要求逐行读取=开发者审查模式，分区地理给普通用户一次性心智模型"上面随便动、下面别碰"。`direct:` `models.rs` SafetyLevel 两判据串联天然是有序纵深；折叠 Risky 同时物理实现"永不预选"+"红色稀缺化"。
**Rationale:** 三通道编码从"合规地存在"升级为"结构性地不可忽视"；对开发者，分区即分组审查单元，密度不降反升。三级安全模型作为差异化资产的第一次视觉兑现。
**Downsides:** impeccable 三个雷——① "Safe 顶部明亮"**不能翻译成绿色染底**（三个彩色色带分区=dashboard slop），分区靠位置+标题+展开态表达；② Risky 红边**严禁侧边色条**（绝对禁令），用 1px 完整低饱和红描边或红色文字标签+图标；③ 与 survivor 6 去冗余：分区已承载安全等级，行内就别再挂彩色徽章。Risky 保持一次点击可展开，别藏到审计不到的深度。
**Confidence:** 78%
**Complexity:** Medium

### 6. 渐进披露单形态·展开=换一个问题（含等价 CLI 命令）

**Description:** 不做 Simple/Advanced 模式开关，而是同一套 DOM、两副语义面孔、恰好两层：**折叠**答普通用户的"值不值删"（分类名 + 体积条 + 证据短句 + 右对齐等宽体积，≤4 元素）；**展开**换一副面孔答开发者的"它到底是什么"（完整路径可点开 Finder + 命中规则名与 root_marker + impact/recovery 全文 + 一行**可复制的等价 CLI 命令** `mc clean --only xcode-derived`）。切分轴不是"密度耐受"而是"在问不同问题"。

**Axis:** E
**Basis:** `external:` NN/g 渐进披露硬约束（≤2 层、切分要准、入口显而易见）；Raycast"卡片行+键盘层"双面性。`reasoned:` 现 GUI 失败在两层显示同种信息（路径清单）的不同数量，切分维度选错；正确切分是"决策信息 vs 审查信息"。等价 CLI 命令把"GUI 用户升级为 CLI 用户"变成渐进披露出口，呼应战略"开发者已被 CLI/TUI 服务"——GUI 不必留住开发者，体面送回终端即成功。
**Rationale:** 这是 C1"不做硬开关"唯一优雅的兑现路径；规则表 impact/recovery/root_markers/规则名数据早已齐备，纯呈现层。
**Downsides:** 被否的 "PR 双 tab"框架正是字面模式切换（两个可切 tab = 两套模式），故**只取"展开=换问题"内核、弃双 tab 外壳**。展开态别做成 key-value 表单墙；折叠行预算 ≤4 元素否则 badge spam；展开允许多开（开发者要对比），150–200ms ease-out。
**Confidence:** 82%
**Complexity:** Medium

### 7. 补 purge/uninstall 功能缺口 + 可见导航 + Cmd+K 加速器

**Description:** GUI 现只注册了 clean/analyze，`mc-core` 的 `scan_purge`（开发产物）与 `scan_uninstall`（应用卸载残留）**完全没有 GUI 入口**——而"卸载软件删干净"正是 CleanMyMac/AppCleaner 招牌、普通用户最痛的需求。接入这两条能力，用**可见的顶部导航**（现双 tab 扩成 3–4 项）承载，再叠一个 Raycast/Linear 式 **Cmd+K 命令面板**作开发者加速器。

**Axis:** A
**Basis:** `direct:` `lib.rs:62-71` 核实——`invoke_handler!` 只注册 clean/cancel_scan/analyze/classify_marked/delete_marked/permission，无 purge/uninstall，真实功能缺口；CLAUDE.md 载明 Engine facade 有 `scan_purge`/`scan_uninstall`。`external:` 研究 C5（Raycast/Linear Cmd+K 差异化）。
**Rationale:** 这不是视觉打磨，是已验证的**功能性缺口**——普通用户最痛的需求 GUI 层完全不可达，只能用 CLI（普通用户不会）。补上比任何视觉打磨都更直接地扩大 GUI 可服务人群，正面对位 Mole。
**Downsides:** impeccable IA 越位警示——Cmd+K 对普通用户不可见，**只能是加速器不能是 purge/uninstall 唯一入口**，须有可见导航承载（别加侧边栏）；半成品面板比没有更糟（模糊匹配/焦点陷阱/与全局 modal 视觉一致，缺一别上）。basis 验证器：Analyze 是刻意独立于 ProgressReporter 的"浏览 vs 自动预选删除"心智模型，**统一的是入口菜单不是结果集**（勿字面合并 clean+purge+analyze 为一个动作）。
**Confidence:** 85%
**Complexity:** Medium-High

## 方向综合与推荐

七个 move 不是七选几，而是**咬合成一条论证**：稳定(1) → 给答案(2) → 分级信任(4) → 空间地理(5) → 无弹窗(3) → 渐进披露(6) → 补全能力(7)。据此分层：

| 方向 | 组成 | 定位 | 裁决 |
|---|---|---|---|
| **A · 务实基座** | move 1 + 3 + 4 + 修终端残留物 | 只解决"跳变+难看+信任"，保留现双 tab IA，低风险 | **B 的第一施工段，不是备选** |
| **B · 普通用户优先主线** | move 1–7 全部 | 整套重设计：新手默认屏 + 渐进披露 + 安全地理 + 补缺口 | **✅ 推荐** |
| **C · 空间总览 + 菜单栏 HUD** | 打开即缓存态刷新 + 菜单栏常驻概览 | 补品类基本盘（DaisyDisk/CleanMyMac/macOS 储存空间都有） | **⏸ 推迟到独立一轮**（更大工程面，须轻量实现） |

- **为什么 A 不能单做**：impeccable 评审点破——A 治好跳变却治不好病根（诊断 §1.3：病在"DESIGN.md 把 GUI 定义为 TUI 移植"这个定位），A 的产物是"一个更顺滑的 TUI 克隆"，全等宽文本网格的"难看"依然在，slop test 依然过不了。但 A 的三个 move 是纯结构/低风险，**应作为 B 的第一个可交付里程碑**先落地止血。
- **为什么推荐 B**：七 move 互相支撑（没有 move 2 的首屏答案，move 4 的分区就悬空），设计风险集中在各 move 的 impeccable 护栏点（数字滑向营销、chip 滑向 slop、三色分区、徽章自夸），每条都有明确护栏、可控。
- **为什么 C 推迟（不是否决）**：空间总览、菜单栏概览是品类基本盘功能，缺了是短板不是优点——**该做**。推迟的真实理由只有两条，都关于"怎么做/何时做"而非"做不做"：① **工程面**：菜单栏常驻 + 后台刷新碰 app 生命周期/常驻进程，超出本轮"只改呈现层"范围，需单独一轮 brainstorm 定形态（常驻 vs 按需、菜单栏 vs 窗口内）；② **明文约束**：须满足 `STRATEGY.md` "轻量、不成为系统负担"——约束的是实现（常驻要轻、无角标无通知无高频后台扫描），不是禁止该功能。且其"空间总览可视化"大半已在 move 2（分段横条/可选 treemap），真正新增面只有菜单栏 HUD。**不因"竞品有/要不一样"而砍**（那条推理已作废，见文首纠错声明与 `docs/solutions/product-decisions/differentiation-on-execution-not-opposition.md`）。

## 设计系统原则（impeccable，跨 move 必守）

1. **字体管辖权切割**：mono 只给数据（路径、体积、CLI 命令），标签/标题/证据文案一律系统无衬线；字阶 1.125–1.2、全局 ≤3 级，move 2 大数字是唯一例外且靠 weight 非 display 尺寸。这是从"TUI 克隆"到"产品"的单条最大杠杆。
2. **红色跟随语义、永不装饰**：`--state-danger` 只与 Risky/不可逆动作共现；色通道只落小指示物，永不染分区背景。沿用现有 token 层安全轴/状态轴分离。
3. **布局物理学**：外壳三区稳定性下沉 route——相位切换是槽位内容替换，区块永不 mount/unmount；流式数字 tabular-nums + 固定列宽；扫描期行序锁死，排序在完成时一次 settle。
4. **动效只传达状态**：150–250ms ease-out，仅限填充、展开、一次性 FLIP settle、toast 进出；无入场编排、无 settle 后 count-up、**全 app 无 spinner**（进度=正在累加的数字和正在填充的条本身）；reduced-motion 瞬切（tokens.css 已有）。
5. **披露两层硬顶 + 行内预算**：折叠行 ≤4 元素，证据是弱化文字不是彩色药丸；密度是给开发者的许可而非对所有人的义务——默认屏永远先给一个答案，再给一张表。

## 复用基建层（B 的"一次做对"杠杆）

这些不是独立 move，是让 B 可维护、四类页面继承的原语（多在上面的 survivor 里体现）：`tokens.css` 扩展（间距/动效/海拔，扩展非推翻）· `Shell+Slot` 布局原语（survivor 1）· `StreamingList`（survivor 1，复用限 Svelte 路由）· `EvidenceCard`（survivor 4）· `DestructiveAction` 安全状态机（把 `selected = safety != Risky && preselect` + type-to-confirm + Trash 封装成不可绕过原语，`ConfirmDelete.svelte:20-30` 已有雏形）· `Row+Disclosure`（survivor 6）。

## Rejection Summary

| # | 候选 | 砍/降级理由 |
|---|------|------|
| 10 | 拆掉全部确认弹窗 | 字面违反 Risky type-to-confirm 硬约束；限定非 Risky 则与 survivor 3 重复 |
| 15 | 去掉 Onboarding 教程页 | **误读**：`Onboarding.svelte:41-51` 是必需的 FDA 完全磁盘访问授权流程，非教程，无法用证据文案替代 |
| 30 | GitHub PR 双 tab | 两个可切 tab = 字面 Simple/Advanced 模式切换，违反 C1；内核已并入 survivor 6 |
| 33 | Stale-While-Revalidate 常驻仪表盘 | **未否决，推迟**：碰 app 生命周期超出本轮呈现层范围，须单独一轮 + 轻量实现（≠"竞品有故砍"）。"打开即缓存态刷新"这半不需常驻、可较早并入 |
| 39 | 菜单栏为主入口 | **未否决，推迟**：菜单栏常驻是新增工程面，须满足 STRATEGY 轻量约束（无角标/通知/高频扫描）；是品类基本盘该做，只是排在呈现层止血之后 |
| 29 | 银行对账单式持久本地账本 | 跨会话持久化超出呈现层范围（= 已有 `mc history` 引擎活） |
| 27 | 游戏装备稀有度色系 | 语义错位（稀有=珍贵 vs 危险=避免），仅换皮无新机制；survivor 5 空间地理是更好形式 |
| 14 | 单一动作跑齐 clean+purge+analyze | 忽视 Analyze 刻意独立于 ProgressReporter 的"浏览 vs 自动预选删除"架构；统一的是入口非结果集 |
| 25 | 收件箱分流(Risky=垃圾邮件文件夹) | 与"≤2 层、不给次级项多入口"张力；survivor 5 地理是更优形式 |
| 09 | 消灭相位本体 | 状态机不可消灭；是 survivor 1 诊断的不精确激进表述 |
| 13 | 扫到即可处理 | 大部分已实现（扫描期 checkbox 已可勾选，仅删除按钮被 gate），novelty 高估 |
| 20/21 | PresentationBatcher/StreamingList 与 TUI 共享 | Rust/ratatui 与 Svelte/浏览器运行时不同，只能共享契约/思路，非代码；复用限 Svelte 路由（已并入 survivor 1） |
| 12/31/26/35 | 各类"分段条/地铁图/到达板/注水槽" | 与 survivor 1/2 同机制的不同类比标签，已合并（"8 帧收敛"计数含虚高，属同一想法重复计数） |
| 16 | 删除前"预演"磁盘 Y→Z | 需当前不存在的"磁盘总容量/剩余"数据源，轻量新增；可作 survivor 2 的增强项延后 |
| 17/19/22/23/24 | 各复用原语 | 非独立 move，已归入"复用基建层"与对应 survivor |
