---
date: 2026-07-05
topic: beat-mole-product-directions
focus: 非性能演进方向；坐标系=打赢 Mole（免费GUI/更好体验/更可信/更开放）
mode: repo-grounded (surprise-me)
grounding: 学习检索(docs/solutions) + 竞品外部调研(ce-web-researcher) + 6帧发散 + Codex 独立对抗式审查(读代码核实现状 + 核实 Mole)
---

# Ideation: macCleaner 非性能演进方向（打赢 Mole）

> 用户诉求（原话）："我们这个其实一开始就是模仿 mole 做的，我个人觉得 mole 的体验并不是太好，比较慢，而且 GUI 还收费，想做个更好的。"
> 性能不在本次范围（另有会话覆盖）。本轮聚焦非性能的产品方向。
> **本文结论经 Codex 独立审查纠正**：多项原判定被证伪或降级——见下方"追平 vs 打赢"标注与拒绝摘要。

## Grounding Context（Codebase + 竞品 + Codex 核实）

**产品现状（代码核实）：**
- CLI 四命令 Clean/Uninstall/Analyze/Purge（`crates/cli/src/main.rs:27`）。clean 6 条规则/2 分类（浏览器缓存+系统缓存，`clean_rules.toml`）；purge 13 类开发产物。
- 规则编译期 `include_str!` 锁死（`rules.rs:141/147`），无用户扩展路径。`rules.rs` 契约测试已是安全 gate（`purge_rules_safety_levels`/`dirname_rules_have_guards`/`no_rules_reference_user_data_paths`）。
- 删除移废纸篓可恢复（`models.rs:26`/`cleaner.rs:22`）；`CleaningDone.deleted_paths` 只含成功删除项（`cleaner.rs:49`）；无 `history`/`undo`。
- TUI 已有 `AnalyzeEvent`+`IncrementalTreeBuilder`+置换底座+统一标记集（`lib.rs:527`/`tree_builder.rs`），Analyzer 可标记删除+回查规则证据。CLI analyze 是另一套深度2同步树+大文件列表（`analyze.rs`）。
- GUI 仅是产品愿景（STRATEGY/PRODUCT），workspace 只有 core/cli/tui。视觉语义 token + 桌面基准已文档化（DESIGN.md/theme.rs）。
- **现存安全债（Codex 挖出）**：`app_resolver.rs` 把 `Application Support/WebKit/HTTPStorages/Saved Application State` 等可能含用户数据的残留标 **Safe 且默认勾选**（`app_resolver.rs:26/276`，测试锁死"全部 Safe" `:530`）。按 `models.rs:4` SafetyLevel 文档，可能丢不可再生用户数据应为 **Risky**——这是当前的模型违背，是 #5 的前置修复项。

**竞品坐标系（Codex 核实 Mole 现状，纠正原判）：**
- Mole（tw93，~58k stars）：CLI **GPL 开源、有 CONTRIBUTING**；仅 **Mac App 收费**（官网 $19/2台/永久）。已有 `mo history` + 操作日志、`mo clean`（含**已卸载 app 残留**）、`mo analyze`+大文件+treemap、`mo status`、Raycast/Alfred launcher、JSON、**SECURITY_AUDIT + attestation**。
- 推论：**Mole 不是弱对手**。原列表里 history / analyze大文件 / orphan残留 都是 Mole 已有 → **追平，非打赢**。"Mole 规则封闭"叙事**错误**——正确差异是"签名数据集 + CI 契约 + 可审计"，不是"它封闭"。
- CleanMyMac 被骂 snake-oil / maintenance scripts / RAM purge placebo（明确避开）。重复文件已被 czkawka/fclones/rmlint 做透（不自建，作 analyze 视图）。Homebrew-core 早入=低成本高杠杆分发。

## Topic Axes
Decomposition skipped — surprise-me mode

## Ranked Directions

> 每项标注 **[打赢]**（差异真实存在于 Mole 空位）或 **[追平]**（Mole 已有，做它是补短板/表 stakes）。排序采用 Codex 修正后的优先级——真差异化在前，追平项靠后。

### 1. 免费开源 GUI（MVP，复用 core/TUI 安全语义） — [打赢]
**Description:** 做一个免费、开源、体验精良的 GUI，直击 Mole 唯一付费点（Mac App）。范围**限定为 MVP**：复用现有 mc-core 引擎 + TUI 已抽象的框架无关语义 token/置换底座，完整继承 SafetyLevel 三通道编码、Risky type-to-confirm、默认 Trash、无遥测声明。两条路取舍：**自建 GUI**（控制体验，工程面大）vs **SDK 化 mc-core** 让 Raycast/Alfred 等做前端（分发广但只是 power-user launcher，Mole 已有 launcher，差异弱）——Codex 判断 SDK 化不能替代 GUI。
**Basis:** `direct:` GUI 仅愿景(STRATEGY.md:14/PRODUCT.md:15)、语义token已文档化(DESIGN.md/theme.rs:75) · `external:` Mole 官网核实 Mac App 商业化、README 说明是 separate proprietary app。
**Rationale:** 这是唯一"Mole 收费你免费"的正面对位，也是最大产品杠杆。
**Downsides:** 最大工程面；必须完整复制 TUI 全部安全语义，不能为"好用"把 Risky 做成一键清理。
**Confidence:** 80% · **Complexity:** High · **Status:** Unexplored

### 2. 信任/开放层：本地规则叠加 → 签名数据集 → CI 契约 + 安全资产 — [打赢]
**Description:** 把"规则外部化"与"可信叙事"合并成一条链。①本机 `~/.config/mc/rules.toml` **只读叠加**（严格 lint、禁止用户数据路径、禁止降级 safety、默认 disabled 能力分级）→ ②签名可加载规则数据集（热修一条误报从"发版+等 Homebrew"降到"改一行 TOML"）→ ③社区 PR + CI 契约门禁（复用 rules.rs 契约测试机器把关）。**并列做信任资产**：公开安全边界文档 + 规则审计报告 + 路径保护测试 + release checksum/attestation + **规则透明度页**（每条规则展示依据/风险/恢复/测试状态/贡献入口）。
**Basis:** `direct:` 规则 include_str! 锁死(rules.rs:141)、契约测试已存在(rules.rs:203/351/421) · `external:` winapp2.ini/EasyList 社区规则数据集模式；Mole 已有 SECURITY_AUDIT+attestation（要"更可信"需对等物）。
**Rationale:** "更开放 + 更可验证" vs Mole 专有 App，是可证的差异化，而非"它封闭"的伪叙事。信任是复利资产（越多审计眼睛越可信）。
**Downsides:** 最大风险=用户规则突破安全模型（自定义 Safe+preselect 指向 Documents 会毁信任）→ schema/能力分级/签名源/lint 是必答题。信任资产是持续投入。
**Confidence:** 80% · **Complexity:** Med（分级摊薄） · **Status:** Unexplored

### 3. 权限/Full Disk Access 失败解释与引导 — [打赢]（低争议高价值）
**Description:** 清理工具成败常在"为什么扫不到/删不了"。做清晰的 FDA/权限状态检测 + 失败归因（哪条路径因权限跳过、如何授权）+ 引导。而非静默漏扫。
**Basis:** `reasoned:` 权限失败是清理工具最高频的隐形失败点，比重复文件更接近"信任"这个核心；Codex 独立指出此空白。
**Rationale:** 直接服务"放心/可信"情绪目标，且几乎不触碰安全模型、争议低、用户价值具体。
**Downsides:** 需处理各类 macOS 权限边界与版本差异；引导文案要克制不吓唬。
**Confidence:** 72% · **Complexity:** Low-Med · **Status:** Unexplored

### 4. 本地清理历史 + `mc undo` — history[追平] / undo[打赢]
**Description:** 本地 append-only 账本（`~/.local/state`，零遥测）记 path/size/category/时间戳 + trash 后位置。`mc history` 看"上次清理以来"回长趋势；`mc undo <run-id>` 从废纸篓批量还原。**history 本体是追平**（Mole 已有 `mo history`）；**undo 是打赢**（Mole 缺）。
**Basis:** `direct:` deleted_paths 现成但用完即弃(cleaner.rs:49)、Trash 可恢复(models.rs:26) · `external:` Mole `mo history` 已有，无 undo。
**Rationale:** undo 降低"不敢删"门槛→敢勾更多→交付价值更高。
**Downsides:** Trash 不是事务日志——跨卷 Trash/清空废纸篓/同名恢复/权限/部分失败都要建模；`--permanent` 不能 undo，**不要承诺 permanent undo**（Codex）。先做 history/ledger，再做 Trash restore。
**Confidence:** 82%(history)/70%(undo 复杂度) · **Complexity:** Low-Med(history)/Med(undo) · **Status:** Unexplored

### 5. 孤儿残留扫描（反向卸载）——**前置：先修 app_resolver 安全债** — [追平]
**Description:** 扫 ~/Library 找 bundle-id 父 App 已不存在的孤儿。**但先决条件**：先重评级现有 uninstall 残留分级——把可能含用户数据的残留从 Safe+默认勾选改为按 SafetyLevel 决策树正确分级（潜在 Risky/Moderate、不默认勾选）。
**Basis:** `direct:` uninstall 依赖 .app 尚在(app_resolver.rs:35/232)、反向扫描不存在；**现存安全债**(app_resolver.rs:26/276/530) · `external:` Mole `mo clean` 已含已卸载残留（故此项是追平）。
**Rationale:** 填能力缺口 + 顺带还一笔真实安全债。
**Downsides:** 误杀真实用户数据（共享 bundle-id、系统预留目录）；沿用现有 Safe 默认勾选会**违反安全模型**。
**Confidence:** 70% · **Complexity:** Med · **Status:** Unexplored

### 6. Analyze 归因 + 大文件只读增强（重复文件视图最后做，永不预选） — [追平]
**Description:** 复用增量树+置换底座+统一标记集管线：①归因（root_markers/规则反查大目录所有者；未识别大目录一键起草规则草稿喂 #2）②大文件只读视图 ③重复文件视图（**最后做**，内容哈希分组+字节级二次校验+**永不预选**+每副本独立身份键，不做独立 czkawka-clone）。**不喊"空间洞察平台"**（Codex：过度包装，且都是追平 Mole）。
**Basis:** `direct:` AnalyzeEvent+IncrementalTreeBuilder 已抽象可复用(lib.rs:527/tree_builder.rs)；Analyzer 未命中规则路径曾默认 Safe(lib.rs:1329)，该安全债已于 2026-07-10 修复为“仅信内置规则、未知路径 Risky + type-to-confirm”——扩展到重复/大文件时继续保持未知用户文件**不能默认 Safe/选中** · `external:` Mole 已有 disk insights/large files/treemap（追平）；重复文件已被 czkawka/fclones 做透。
**Rationale:** 复用现有底座边际成本低；未识别大目录是 #2 规则库的天然线索来源。
**Downsides:** 归因准确度依赖规则覆盖；重复文件是安全雷区（硬链接/APFS clone/Photos库/包内容/同步盘）。
**Confidence:** 70% · **Complexity:** Med(归因/大文件)/High(重复文件) · **Status:** Unexplored

### 7. 声明式 `.mc.toml` 本机策略（CI/Fleet 暂缓） — [追平]
**Description:** 每仓/全局三态策略（auto 静默清 Safe 到废纸篓 / ask / never），可提交可 review。**先做本机 policy/dry-run**；`mc apply --ci` 与 Fleet **暂缓**到规则成熟后。
**Basis:** `direct:` 无 apply/--ci 命令(main.rs:27)、无 .mc.toml 实现 · `reasoned:` 开发者配置即代码母语；purge 13 类硬编码本质已是"什么可删"判断。
**Rationale:** 对"闭源 GUI 进不了 CI"成立，但对普通清理用户价值弱，故靠后。
**Downsides:** CI 自动清理天然鼓励 --yes/非交互——策略文件**绝不能允许 Risky=auto**；--ci 默认 dry-run 或只允许 Safe/明确 allowlist 的 Moderate；永久删除不可作策略默认（Codex 硬约束）。
**Confidence:** 62% · **Complexity:** Med · **Status:** Unexplored

## Rejection Summary

| # | Idea | Reason Rejected |
|---|------|-----------------|
| 1 | 启动项只读体检 | scope overrun→"系统管理器"，偏离清理同一性 |
| 2 | 独立重复文件工具 | czkawka/fclones/rmlint 已做透；仅作 analyze 视图(#6)且永不预选 |
| 3 | 菜单栏/阈值 launchd 哨兵 | 与轻量/CLI-TUI 定位张力，premature，并入 #1 GUI |
| 4 | Rebuildability Proof（删前证明可重建） | 验证脆弱、大量假阴，过度工程已稳的安全模型 |
| 5 | 留观期/分代 vacuum | macOS atime 不可靠(relatime)，加状态复杂度，性价比低 |
| 6 | Leak Doctor 源头治理 | 改用户工具配置有风险，漂移"配置管理器" |
| 7 | 社交 benchmark | 需数据采集飞轮，与零遥测张力，近 gimmick |
| 8 | 信任相容变现（付费规则包等） | 业务战略问题非产品方向，先有用户再谈 |
| 9 | Safe 自动驾驶/学习预选/无需清理提示/冷仓 purge | feature-detail 或 tactical 小项，已并入 #2/#4/#7 |
| 10 | 恢复演练 UX | 并入 #4 的 undo |
| — | **原判"Mole 规则封闭"** | **Codex 证伪**：Mole CLI 是 GPL 开源+CONTRIBUTING；差异叙事已改为签名数据集/CI/可审计(#2) |
| — | **原判 #1/#4/#5 均为"打赢"** | **Codex 修正**：Mole 已有 history/analyze大文件/orphan 残留，多为追平；已重新标注 |

## 落地建议（Codex 修正后的优先级）
1. **免费开源 GUI**（MVP，复用现有 core/TUI 安全语义）
2. **规则外部化第一阶段**（本地只读叠加 + 严格 lint + 禁降级 safety）
3. **信任资产**（安全审计文档 + release 校验 + 规则契约 CI + 权限失败解释）
4. **history + undo**（先 history/ledger，再 Trash restore；不承诺 permanent undo）
5. **孤儿残留**（先重评级 app_resolver 残留风险，再做 30 天以上孤儿候选）
6. **Analyze 归因/大文件只读增强**
7. **`.mc.toml` 本机策略**；CI/Fleet 暂缓
8. **重复文件**，最后做，且永不预选
