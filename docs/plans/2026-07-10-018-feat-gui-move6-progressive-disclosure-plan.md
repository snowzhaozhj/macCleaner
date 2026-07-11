---
title: macCleaner GUI move 6 渐进披露·展开=换问题（审查面孔） - Plan
type: feat
date: 2026-07-10
topic: gui-move6-progressive-disclosure
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-brainstorm
execution: code
---

# macCleaner GUI move 6 渐进披露·展开=换问题 - Plan

## Goal Capsule

- **Objective:** 把 Clean 列表的展开从"同种信息的更多逐项"改为 move 6 的"**换一个问题**"——折叠答普通用户"值不值删"，展开换一副面孔答开发者"它到底是什么"（完整路径可点开 Finder + impact/recovery 全文 + 可复制的现存等价命令）。恰好两层（NN/g 渐进披露硬约束）。
- **Product authority:** 用户（产品负责人）＋ `STRATEGY.md`（普通用户优先 + 渐进披露保开发者）＋ `docs/ideation/2026-07-07-gui-redesign-ideation.md §6`（move 6 内核）＋ 计划 015（路线图）。
- **Open blockers:** 无。计划期岔口已定夺（见 Key Decisions 路 B）。

> **来源核实（grounding）：** 当前 `StreamingList.svelte` 已是两层（分类头 → 展开逐项 rows：checkbox + `Safety` + `PathText` + 单行 `EvidenceCard` + size），但**展开的逐项仍是"决策形态"**（单行截断证据），未换成"审查形态"；分类折叠行只有 count + size，无体积条。核实到两处硬约束：① CLI **无** `--only`/规则过滤参数，`CleanRule` 只有中文 `name`/`category`、**无稳定 slug**——逐项精确等价命令（`mc clean --only xcode-derived`）**目前无法诚实生成**；② `Found` 契约（`progress.rs:10`）带 category 但**不带 rule name**；③ "在 Finder 显示"**无前端 opener API**（未装 `@tauri-apps/plugin-opener`，capabilities 仅 `opener:default`），现有 reveal 手段是后端命令（`open_trash` 用 `tauri_plugin_opener::open_path`）。

---

## Product Contract

### Summary

把 Clean 的 `StreamingList` 展开态从"逐项决策行"升级为"**审查面孔**"：折叠层（分类行）用分类名 + 体积占比条 + count/size 答"值不值删"；展开层每个逐项呈现完整路径（可复制）+ impact/recovery **全文** + "在 Finder 中显示"，分类展开区顶部给一行可复制的现存等价命令 `mc clean`（把 GUI 用户体面送回终端的出口）。纯呈现层复用 `mc-core`，删除的安全语义完全不变；后端仅加一个与 `open_trash` 同款的加性命令 `reveal_in_finder`。

### Problem Frame

现 GUI 的"展开"在两层显示**同一种信息（路径清单）的不同数量**——切分维度选错（"密度耐受"而非"在问不同问题"）。move 6 的正确切分是"决策信息 vs 审查信息"：折叠答"值不值删"，展开答"它到底是什么"。等价 CLI 命令把"GUI 用户升级为 CLI 用户"变成正式出口，呼应战略"开发者已被 CLI/TUI 服务，GUI 不必留住开发者、体面送回终端即成功"。

### Key Decisions

- **等价 CLI 走路 B（纯前端 + 现存命令），不假造。** CLI 无 `--only`、规则无 slug，逐项精确命令无法诚实生成；本 PR 用**现存真实命令** `mc clean` 作为分类/整体层面的等价出口，**逐项不放不精确的命令**（诚实招牌不破）。精确 `--only`（需先建规则 slug + CLI 参数）诚实延后为独立 PR。
- **"在 Finder 显示"用加性后端小命令 `reveal_in_finder`。** 与 `open_trash` 完全同款（复用 `open_path`），不碰引擎/扫描/删除逻辑；不引入前端 opener 包与 capabilities scope 扩张。
- **命中规则名延后。** `Found` 无 rule name，改契约成本大、价值边际（category + 证据全文已表达"它是什么"）。
- **本 PR 只做 Clean。** Analyze 的审查面孔可诚实提供 `mc analyze <path>` 作为“从该路径继续分析”的 CLI 出口；`mc purge <path>` 会扫描后进入交互清理，`mc purge <path> --dry-run` 才是只读预览，二者都**不等价于删除任意 Analyze 项**，不得标成等价删除命令（如展示，只能分别标注“交互清理此处的开发产物”/“预览此处的开发产物”）。Analyze 是树导航、展开交互需单独设计——列为 move 6 第二段（下个 PR），避免混入。
- **恰好两层，只改展开语义、不加模式开关。** 不做 Simple/Advanced 硬切换；复用现有 `expanded` 状态机。

### Actors

- **A1. 普通 Mac 用户（首要）** — 折叠层即可判断"值不值删"，无需读路径清单。
- **A2. Mac 开发者（次要）** — 展开换到审查面孔：看完整路径、点开 Finder 核对、复制等价命令回到终端。

### Key Flows

- **F1. 决策（折叠，默认）。** 扫描完成 → 分类行（名 + 体积占比条 + count/size）→ 普通用户据此一键清理，全程不展开。
- **F2. 审查（展开）。** 点分类头展开 → 顶部一行可复制 `mc clean` + 逐项审查面孔（Safety + 完整路径可复制 + impact/recovery 全文 + 在 Finder 中显示 + size + 复选框）→ 开发者核对/复制命令/调整勾选。

### Requirements

**折叠层：答"值不值删"（≤4 元素）**

- R1. 分类折叠行呈现：分类名 + 体积占比条（该分类占已选/总量比例）+ count + size；元素预算 ≤4，避免 badge spam。
- R2. 折叠行保持防跳变契约不变（keyed 行、扫描期骨架、完成时一次 FLIP settle）——move 6 不回退 v1 稳定性。

**展开层：审查面孔，答"它到底是什么"**

- R3. 展开区顶部呈现一行可复制的现存等价命令 `mc clean`（点击复制到剪贴板 + 复制反馈）；文案标注这是"用命令行清这一批"的出口，不声称逐项精确。
- R4. 每个展开逐项呈现审查形态：Safety 三通道 + 完整路径（可复制）+ impact/recovery **全文**（不截断，可换行）+ "在 Finder 中显示"按钮 + size + 复选框。
- R5. "在 Finder 中显示"调用加性后端命令 `reveal_in_finder(path)`（复用 `open_path`）；失败优雅降级（提示而非静默/崩溃）。
- R6. 展开态非 key-value 表单墙；允许多个分类同时展开（开发者对比）；展开动画 150–200ms ease-out。

**安全语义（不变量，完全继承）**

- R7. 复选、预选、删除路径、type-to-confirm、默认 Trash 全部不变；move 6 只增审查呈现与 Finder/复制出口，不改任何删除决策路径。
- R8. `reveal_in_finder` 只读揭示，不删除、不移动、不改文件；仅 fork 系统 `open`。

### Success Criteria

- 折叠分类行有体积占比条且元素 ≤4；普通用户不展开即可决策。
- 展开逐项为审查形态：完整路径 + 全文证据 + 在 Finder 中显示 + 复制路径均可用；点"在 Finder 中显示"触发 `reveal_in_finder` 携正确路径。
- 展开区顶部 `mc clean` 可复制且有反馈；无任何不精确/不存在的命令展示。
- 删除安全语义与 main 一致（Risky type-to-confirm、默认 Trash）无回退。
- `pnpm check`/`build`/`test`/`e2e` 全绿（clean.spec 展开断言按需更新，不放宽安全断言）；`cargo clippy --all-targets`(pedantic) 无警告 + `cargo test` 通过（含新命令）。

### Scope Boundaries

**In scope**
- `crates/gui/frontend/src/lib/StreamingList.svelte`（折叠行占比条 + 展开审查面孔）。
- 新增/复用小组件：审查行、复制按钮（可抽 `CopyButton`/`ReviewRow`，带单测）。
- 后端 `reveal_in_finder` 命令 + `lib.rs` 注册 + `ipc.ts` 封装 + 必要 capabilities。
- clean.spec/单测更新。

**Out of scope（诚实延后）**
- 逐项精确等价命令 + CLI `--only` + 规则 slug 体系（独立 PR）。
- 命中规则名/root_marker（需改 `Found` 契约）。
- Analyze 审查面孔（move 6 第二段；可提供精确 `mc analyze <path>`，无任意路径删除的等价 CLI）。
- move 7 / 真 undo / 仪表盘 / 后端增量树流式。

### Outstanding Questions（已在实现中定夺）

- **复制反馈形态** → 抽 `CopyButton` 原语（路径 + 命令共用），点击后 1.5s 内按钮内联显示「已复制」/失败「失败」，用 `navigator.clipboard.writeText`（点击手势触发，WKWebView 允许），不引入 clipboard 插件/capability。
- **占比条口径** → 占**总命中体积**之比（与分类行显示的 size 数字同源，读数一致），非已选口径。
- **等价命令位置** → 一次呈现于列表顶部（非每分类重复），诚实标注「清理全部可安全释放项」；逐项精确 `--only` 因 CLI 未支持而不展示（不假造）。

### Implementation Summary（本 PR 实际改动）

- `crates/gui/src/commands/reveal.rs`（新）— `reveal_in_finder(&str)`：macOS `open -R` reveal-and-select，路径不存在返回 Err（不静默）；`commands/mod.rs` + `lib.rs` 注册。
- `crates/gui/frontend/src/lib/StreamingList.svelte` — 折叠分类行加体积占比条；顶部一次呈现可复制等价命令 `mc clean`；展开层升级为审查面孔（Safety + 完整路径可复制 + impact/recovery 全文 + 在 Finder 中显示 + size + 复选框）；Finder 失败落底部横幅。
- `crates/gui/frontend/src/lib/CopyButton.svelte`（新）— 复制原语，1.5s 反馈。
- `crates/gui/frontend/src/lib/EvidenceCard.svelte` — 加 `full` prop（审查态去截断、impact/recovery 分行全文）。
- `crates/gui/frontend/src/lib/ipc.ts` — 加 `revealInFinder` 封装。
- `crates/gui/frontend/e2e/{contract.test.ts,support/fixtures.ts,clean.spec.ts}` — 契约守卫命令数 9→10 + `reveal_in_finder` mock + move 6 展开审查 e2e。

### Verification（全绿）

- `pnpm check` 0 error；`pnpm build` 通过；`pnpm test` 43 passed；`pnpm e2e` 17 passed（新增 move 6 展开审查：完整路径 + `mc clean` + 在 Finder 中显示触发 `reveal_in_finder`）。
- `cargo clippy --all-targets`(pedantic) 无警告；`cargo test` 全通过（core 102 / tui 68 等）。
