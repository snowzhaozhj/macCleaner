---
date: 2026-07-07
topic: next-step-tui-vs-gui
focus: 下一步方向决策——继续深挖 TUI，还是进军 GUI？（对已有探索做排序综合，非重新发散）
mode: repo-grounded
grounding: 复用既有 4 篇 ideation（3 篇 TUI + 1 篇 beat-mole 战略）+ git log/plans 核实"已出货 vs 未动"+ Explore 子代理对抗式核实关键安全项与 GUI 现状
verdict: 进军 GUI（MVP）为推荐主线；TUI 深挖已入收益递减，不再是最高杠杆
---

# Ideation: 下一步做什么——TUI 深挖 vs 进军 GUI

> 用户诉求（原话）："看下下一步做什么比较合适，是继续扩展 TUI 的功能，还是开始进军 GUI 呢？"
> 本轮**不重新发散** 48 个点子——TUI 与产品方向已被 4 篇 ideation 穷举过（见下"已有探索"）。本轮的价值是：核实"哪些已出货、哪些仍空"，据此把这个**分叉决策**排序综合成一个可执行的下一步。

## 已有探索（本轮的输入，不重复）

| 文档 | 结论要点 | 现状 |
|---|---|---|
| `2026-07-04-tui-ux-maturity.md` | 对齐 dua-cli 的 7 项 TUI 成熟化（删除闭环/统一 keymap/help/实时排序/翻页/确认路径清单） | **大多已出货**（commit `64ce226` 及后续） |
| `2026-07-04-tui-interaction-round2.md` | 成熟化之后暴露的 6 项：删除拆树、误标 54GB 陷阱、Uninstall 冻结、键位分叉、退出契约 | **核心已出货**（`d7ed16e` 统一交互 + 扫描中标记；plan 008 round3） |
| `2026-06-03-p1-perf-ux-ideation.md` + perf 系 | 扫描性能（并发/增量/park 引擎） | **已出货**（`12e1f58` park 引擎，扫描期 CPU 降 ~2×） |
| `2026-07-05-beat-mole-product-directions.md` | Codex 审查后的 7 个产品方向，**#1 = 免费开源 GUI** | 见下"逐项现状" |

**beat-mole 7 方向的逐项现状（git log/plans 核实）：**

| beat-mole # | 方向 | 现状 |
|---|---|---|
| 1 | **免费开源 GUI（MVP）** | **完全未开始** ← 唯一大杠杆仍空 |
| 2 | 规则外部化 + 信任/开放层 | **已出货**：用户叠加规则 + 运行时安全 lint（#22）、规则透明度页 + 安全边界 + release attestation（`0ab0a75`） |
| 3 | 权限/FDA 失败解释 | **已出货**：权限跳过结构化 + `mc doctor` 只读诊断（#23） |
| 4 | history + undo | **半程**：`mc history` + 只读账本已出货（#24）；`undo` 仍空（打赢项） |
| 5 | 孤儿残留（前置修 app_resolver 安全债） | **前置已修**：USER_DATA_SUBDIRS 残留改 Moderate + 不预选 + 回归测试（plan 007）；反向扫描本体仍空（追平项） |
| 6 | Analyze 归因 / 大文件 / 重复文件 | **未开始**（追平项） |
| 7 | `.mc.toml` 本机策略 | **未开始**（追平项） |

## Grounding Context（对抗式核实，Explore 子代理，2026-07-07）

三件"会推翻推荐"的事已核实（file:line 见子代理报告，此处摘结论）：

1. **数据丢失级安全项 = 已修复**。round2 #1 的两个致命面都闭合：
   - 删除后不再拆树——分析器删除走 `start_cleaning_from_analyzer` → `restore_analyzer_after_delete`，**原地剪除已删节点并留在树内**（`lib.rs:1388/1476`），不再 `back_to_menu`。"删除后莫名退出"消失。
   - 误标最大项——标记从"按裸索引"改为**按路径 WYSIWYG**（`path_at_display_index`，`lib.rs:1756`），把实时重排的 TOCTOU 降为安全 no-op。曾误标 ~/Library 54GB 的静默错标机制被根治。
   - **诚实残留**：标记键不置 `user_navigated`，未导航时光标停 0 仍会标"当前最大项"——但现在是 WYSIWYG（所见即所标），且标记≠删除（需再确认）+ 废纸篓可恢复 + Risky type-to-confirm 三重兜底。属**小尾项**，非阻塞。
2. **GUI = 一行代码都没有**。workspace 仅 core/cli/tui/xtask，全仓零 GUI 依赖（tauri/egui/iced/slint/dioxus/wry 均无命中）。架构刻意预留"core 零 UI 依赖 + 未来加 crate"的路子（v1 plan 明写"未来 Tauri GUI 只需再加一个 crate"）。
3. **app_resolver 安全债 = 已修复**（Moderate + preselect=false + 单测锁定）。

## 分叉的真实形状：不是纯二选一

关键洞察（TUI 两轮 doc 都点到）：**TUI 的基础重构本就是"为 GUI 轨道铺路"**——共享 chrome / 统一交互内核 / 框架无关的语义 token（DESIGN.md/theme.rs）/ 置换底座 / 统一标记集，都是 core 与 UI 分层的砖。地基已相当程度铺好。所以问题不是"TUI 还是 GUI"，而是：

> **继续在 TUI 层加边际功能（收益递减），还是把已铺好的地基兑现成 GUI（唯一未开采的大杠杆）？**

## Verdict：进军 GUI（MVP），TUI 转维护/尾项

**理由（证据链）：**

1. **TUI 已过成熟拐点，进入收益递减。** 三轮成熟化（round1→round2→round3）+ 鼠标，抱怨逐轮变边际（round4 会是"进度条亚格精度/主题美化"这类镀金）。致命安全项已闭合。**主要用户（开发者）已被 CLI+TUI 充分服务。**
2. **GUI 是唯一"打赢 Mole"的正面对位。** Mole CLI 开源、Mac App 收费（$19）。我们免费开源 GUI 直击其唯一付费点——beat-mole 分析里经 Codex 核实的**最大产品杠杆**，且是触达 STRATEGY.md **二次用户（普通 Mac 用户）** 的唯一手段。CLI/TUI 到不了这群人。
3. **地基已就位、风险可控。** core 零 UI 依赖 + 安全语义（SafetyLevel 三通道编码、preselect 解耦、Risky type-to-confirm、默认 Trash、无遥测）已在 TUI 沉淀成可继承的契约；GUI 复用 `Engine` facade + `ProgressReporter` trait 即可，不重造引擎。
4. **战略一致。** STRATEGY.md"多界面适配"轨道明列 GUI；三层界面（CLI/TUI/GUI）是既定方向，前两层已成熟，GUI 是补齐的一环。

**但设护栏（不能为"好用"牺牲安全）：** GUI 必须**完整继承** TUI 全部安全语义——绝不能把 Risky 做成一键清理、绝不能默认预选可能含用户数据项、删除默认移废纸篓。beat-mole #1 的 downside 原文即此。

## Ranked Next-Step Candidates

> 按"战略杠杆 × 是否兑现已铺地基 × 复利"排序。#1-#2 是推荐主线；#3-#4 是并行/尾项；#5 是"若暂不做 GUI"的替代路径（杠杆更低，诚实列出）。

### 1. GUI MVP —— 先 `/ce-brainstorm` 定 MVP 边界，再 `/ce-plan` [推荐主线 · 打赢]
**Description:** 免费、开源、体验精良的桌面 GUI，复用 `mc-core` 引擎 + 继承 TUI 全部安全语义。**范围严格限定 MVP**——先跑通"扫描→分类预览→安全勾选→移废纸篓"最短闭环（对位 Clean/Purge），Analyze 树视图/Uninstall 二期。技术选型（Tauri vs 原生）、MVP 命令覆盖面、安全交互如何在 GUI 语境落地（type-to-confirm 的 GUI 形态）都需 brainstorm 定清。
**Basis:** `direct:` workspace 仅 core/cli/tui/xtask、零 GUI 依赖，core 已零 UI 依赖可复用（Engine + ProgressReporter）；v1 plan 预留"加一个 crate"。`external:` Mole Mac App 收费 $19 是唯一付费点（beat-mole 已核实）。
**Rationale:** 最大杠杆 + 唯一触达二次用户的手段 + 兑现已铺地基。这是产品从"开发者小工具"走向"能打竞品"的关键一跃。
**Downsides:** 最大工程面；必须完整复制安全语义；GUI 设计/前端是新能力面。故**先 brainstorm 收敛 MVP 边界**再动手，避免 scope 失控。
**Confidence:** 82% | **Complexity:** High | **Status:** Unexplored → 建议 `/ce-brainstorm`

### 2. GUI 前置地基：把 core 安全语义显式导出为稳定 SDK 面 [推荐 · 打赢的一部分]
**Description:** 在动 GUI 前（或作为 GUI plan 第一步），把 `mc-core` 当前"给 TUI 用"的隐式契约收敛成**显式、稳定的 UI 无关 API**：SafetyLevel 编码、`selected = safety != Risky && preselect`、Engine facade（scan/clean/dry_run）、ProgressReporter。确保 GUI 与 TUI **消费同一套语义**，而非各自解释——从源头杜绝"GUI 版把 Risky 做松"的漂移。
**Basis:** `direct:` CONCEPTS.md/models.rs 已文档化安全模型；Engine + ProgressReporter 已是解耦点，但契约主要靠 TUI 隐式沿用。
**Rationale:** 这是把 GUI 从"高风险重写"降为"低风险接线"的关键，且对 TUI 也是净收益（契约显式化）。安全是复利资产，一次做对省后患。
**Downsides:** 抽象粒度要设计好；可能与 GUI plan 合并成一步而非独立。
**Confidence:** 78% | **Complexity:** Med | **Status:** Unexplored（可并入 #1 的 plan）

### 3. TUI 尾项收尾（非阻塞，随手做）[追平/收尾]
**Description:** 把 TUI 剩余小尾项一次清掉：(a) 标记键置 `user_navigated=true`，消除"未导航时标记键仍指向最大项"的残留（`lib.rs:1755`）；(b) beat-mole/round 系里未做的镀金项（NO_COLOR/形状编码/主题）按需，属 raise-the-bar，可无限期延后。
**Basis:** `direct:` Explore 核实的残留风险（WYSIWYG 已根治主体，仅剩此小尾）。
**Rationale:** 低成本闭合最后的安全毛边；但**不构成"继续深挖 TUI"的理由**——它是收尾，不是方向。
**Downsides:** 无——但要克制，别把收尾扩张成"TUI round4"。
**Confidence:** 88% | **Complexity:** Low | **Status:** 可随时做

### 4. `mc undo`（history 已有，undo 是打赢项）[并行可选 · 打赢]
**Description:** 若想在 GUI 大工程期间并行攒一个**真差异化**（Mole 有 history 无 undo）：基于已出货的只读账本 + Trash 可恢复，做 `mc undo <run-id>` 批量从废纸篓还原。先做 Trash restore，**不承诺 permanent undo**。
**Basis:** `direct:` `mc history` + 账本已出货（#24）、Trash 可恢复（models.rs:26）；undo 仍空。`external:` Mole 缺 undo。
**Rationale:** 降低"不敢删"门槛，是引擎侧的真打赢项，且与 GUI 正交、可并行。
**Downsides:** Trash 不是事务日志（跨卷/清空/同名/权限/部分失败要建模）；`--permanent` 不可 undo。
**Confidence:** 72% | **Complexity:** Med | **Status:** Unexplored

### 5.（替代路径，若暂不做 GUI）引擎纵深：orphan 残留 / analyze 归因大文件 [追平 · 杠杆更低]
**Description:** 若判断"GUI 时机未到"，替代是继续引擎纵深：孤儿残留反向扫描、Analyze 归因/大文件只读视图。**但这些多是追平 Mole**（`mo clean` 已含已卸载残留、`mo analyze` 已有大文件/treemap），杠杆显著低于 GUI。重复文件永远最后做且永不预选。
**Basis:** `external:` Mole 已有对应能力（beat-mole 核实，均标追平）。
**Rationale:** 诚实列出"继续 TUI/引擎侧"的最佳形态，供对比——但它赢不了竞品，只是补短板。
**Downsides:** 追平不改变竞争格局；orphan 误杀用户数据是安全雷区。
**Confidence:** 65% | **Complexity:** Med | **Status:** Unexplored（不推荐先于 GUI）

## Rejection Summary

| Idea | Reason Rejected |
|---|---|
| 继续 TUI round4（键位/美观/主题深挖） | 已过成熟拐点，收益递减；致命安全项已闭合，剩下是镀金——并入 #3 收尾且克制 |
| 把 GUI 做成"SDK 化让 Raycast/Alfred 当前端" | Codex 已判：只是 power-user launcher，Mole 已有 launcher，替代不了正面 GUI 的差异化 |
| 重复文件视图 | czkawka/fclones 已做透；永远最后做 + 永不预选（并入 #5） |
| `.mc.toml` 策略 / CI-Fleet | 对普通清理用户价值弱，追平项，靠后（beat-mole #7 已排末位） |
| 大而全一次做完 GUI 全命令 | scope 失控风险；#1 已限定 MVP 最短闭环，Analyze/Uninstall 二期 |

## 下一步（路由）

**推荐：`/ce-brainstorm` 定 GUI MVP 边界**——这是本轮结论。GUI 是 High complexity、多产品判断（技术选型/命令覆盖/安全交互 GUI 化）的方向，直接 `/ce-plan` 会漏掉边界决策。brainstorm 定清 MVP 后再 `/ce-plan`。

- 想立刻攒一个引擎侧打赢项并行：`mc undo`（#4）可直接 `/ce-plan`。
- TUI 尾项（#3a）低成本，可随手提一个小 PR，不必走完整流程。
- 若对"GUI 时机"仍有疑虑，可先 `/ce-strategy` 复盘 roadmap 再定——但证据（TUI 成熟 + GUI 是唯一大杠杆 + 地基已铺）已足够支持现在就动 GUI MVP。
