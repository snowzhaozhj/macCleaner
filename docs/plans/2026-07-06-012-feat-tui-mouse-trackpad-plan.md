---
title: TUI 鼠标/触控板交互优化 - Plan
type: feat
date: 2026-07-06
topic: tui-mouse-trackpad
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
product_contract_source: ce-brainstorm
execution: code
---

# TUI 鼠标/触控板交互优化 - Plan

## Goal Capsule

- **目标**：让 TUI 的鼠标/触控板体验可预测——滚动不再"飞行"、能落在期望的行；并支持点击选中/标记。
- **产品裁决权**：用户（直接验收最终 PR）。核心取舍已拍板：启用鼠标捕获、固定滚轮步进、含点击选中/标记、不含滚动条。
- **待解阻塞**：无。剩余为实现细节（滚轮步进行数、点击命中测试算法），交由 planning 决定。

---

## Product Contract

### Summary

给 TUI 启用终端鼠标捕获，由应用自己处理滚轮与点击：滚轮/触控板每格滚动固定行数（跨终端一致、不再乱跳），点击列表行定位光标、点击标记区切换标记。覆盖 Results 与 Analyzer 两个列表状态。

### Problem Frame

当前 TUI **完全不捕获鼠标**：事件线程只读 `Event::Key`，终端也未开启鼠标捕获。因此触控板滚动时，是**终端**把滚动手势翻译成一连串方向键上/下发给应用——惯性滚动产生一大串方向键，光标一次跳过许多行，用户"预测不到当前行的位置"。

这套翻译还**逐终端不同**：iTerm2、Terminal.app 各自有"alt-screen 内滚动是否发方向键"的开关，应用侧无法控制，甚至无法把"滚动产生的方向键"与"真的按住方向键"区分开。结果是同一份代码在不同终端下滚动手感不一致，且无法从代码层根治。

### Key Decisions

- **启用终端鼠标捕获**：进入 alt-screen 时开启 `EnableMouseCapture`、退出时关闭，与现有 raw-mode / alt-screen 的开关及 panic 兜底成对管理。这是唯一能做到跨终端行为一致、可精确调速、且支持点击的方式（对标 lazygit / gitui / btop）。
- **接受丢失终端原生拖选复制**：鼠标捕获会接管鼠标，终端原生"拖选文本复制"失效。这是业界普遍接受的代价——需要时用户可按住修饰键（如 iTerm2 的 Option）临时拖选兜底，且清理工具场景下极少需要复制文本。
- **滚轮固定步进**：每个滚轮/触控板 tick 固定滚动 N 行（常量、可调），不随终端惯性放大。取代"手势→方向键爆发"的不可控放大。
- **复用显示序↔存储序置换层做命中测试**：点击的显示行坐标必须经既有 `size_desc_order` 置换翻译回存储索引再取数据，与键盘导航共用同一取值路径，避免流式重排下的错位。
- **点击复用既有"扫描/建树进行中禁止按位置操作"守卫**：列表实时重排期间，按位置的点击标记与键盘按位置标记一样被禁止，防止误标当下最大项。

### Requirements

**终端与生命周期**

- R1. 进入交互界面时开启终端鼠标捕获，退出时关闭；正常退出与 panic 退出都必须还原终端（不得残留鼠标捕获导致外层 shell 异常）。
- R2. 事件读取需接收并转发鼠标事件（滚轮、按下），与现有键盘事件一并进入主事件循环分发。

**滚动**

- R3. 滚轮/触控板向上/向下滚动一个 tick，列表移动固定 N 行（常量，默认值由 planning 定，倾向 3），跨终端表现一致。
- R4. 滚动移动光标/视口的语义与既有方向键一致（受同样的上/下边界 clamp 约束，不越界、不空滚）。
- R5. 滚动在所有含可滚动列表的状态生效：Results、Analyzer，以及确认清单（confirm）的滚动视图。

**点击**

- R6. 在 Results 与 Analyzer 列表中，点击某个可见行 → 光标定位到该行（等价于把键盘光标移到该行）。
- R7. 点击可见数据行 → 同时切换该行标记（加入/移出统一标记集 `App.marked`），语义与键盘标记键（Space）**完全一致**——可手动标记任意项含 Risky；Risky 保护在于「默认不预选」与「删除需 type-to-confirm」，非禁止手动勾选。
- R8. 点击坐标到数据项的映射必须经置换层翻译回存储索引，并一律走 `.get()` 容错（越界点击为 no-op，不 panic）。

**一致性与守卫**

- R9. `Cleaning` 态忽略所有鼠标事件（与既有"清理中不响应按键"守卫一致）。扫描 / 增量建树进行中的点击标记**按路径绑定 + `.get()` 兜底**，可安全生效，无需额外状态守卫（既有键盘标记已是此语义）；点击/滚轮移动光标时须像键盘一样置 `user_navigated = true`，避免 live 自动跟随覆盖用户导航。
- R10. 点击落在列表区域之外（header / footer / 边框 / 空白）为 no-op。

### Key Flows

- F1. 滚动浏览
  - **触发**：用户在 Results / Analyzer 用触控板或滚轮滚动。
  - **步骤**：应用收到鼠标滚轮事件 → 按方向移动 N 行（clamp 边界）→ 重绘。
  - **结果**：列表平滑、可预测地移动固定行数，光标落点可预期。
  - **覆盖**：R3, R4, R5。

- F2. 点击定位光标
  - **触发**：用户点击某可见列表行。
  - **步骤**：由点击 (col,row) 命中测试算出可见行 → 加视口 offset → 经置换翻译得存储索引 → `.get()` 取项 → 设为当前光标。
  - **结果**：光标跳到点击行；越界点击无效。
  - **覆盖**：R6, R8, R10。

- F3. 点击切换标记
  - **触发**：用户点击行的标记区。
  - **步骤**：命中测试定位到项（同 F2）→ 非 `Cleaning` 态 → 复用既有标记逻辑（`toggle_selection` / analyzer 内联）→ 切换 `App.marked`。
  - **结果**：该行标记状态翻转；`Cleaning` 态不改标记。**手动标记与键盘 Space 完全一致**（含可手动标记 Risky——安全模型的 Risky 保护在于「不预选」与「删除需 type-to-confirm」，而非禁止手动勾选）。
  - **覆盖**：R7, R8, R9。

### Acceptance Examples

- AE1. **Covers R3.** 在含 100 项的 Results 里，触控板快速滑动一下。**Then** 列表按每 tick 固定 N 行移动，不会一次跳几十行；停手后光标停在可预期位置。
- AE2. **Covers R6, R10.** 点击列表第 5 个可见行 → 光标移到该行；点击列表下方空白 footer 区 → 无任何变化。
- AE3. **Covers R7.** 点击某 Safe 项的标记区 → 该项加入待删标记；再次点击 → 移出。
- AE4. **Covers R9.** 扫描进行中点击某行标记区 → 标记生效（按路径绑定，安全）；`Cleaning` 清理进行中点击任意处 → 完全无响应。
- AE5. **Covers R1.** 界面运行中触发一个 panic → 终端恢复正常（无鼠标捕获残留，光标/回显正常）。
- AE6. **Covers R7（键鼠一致）.** 点击一个 Risky 项 → 可被手动标记（与键盘 Space 一致；Risky 保护体现在「默认不预选」与「删除时 type-to-confirm」，非禁止手动勾选）。

### Scope Boundaries

- 不做滚动条（可视 scrollbar / 拖动跳转）。
- 不做鼠标拖选、框选、多选拖动。
- 不做 Menu 等非列表状态的点击（本轮只覆盖列表交互）；若实现顺带极低成本可加，否则不强求。
- 不提供"鼠标捕获开关 / 修饰键回退"配置项（增加复杂度，非本轮价值）。
- 不改变删除安全模型（Trash 默认、Risky 仅 type-to-confirm、`--permanent` 仅 CLI）。

### Dependencies / Assumptions

- 依赖 crossterm 的 `EnableMouseCapture` / `DisableMouseCapture` 与 `Event::Mouse`（已在依赖树中，crossterm 已是 TUI 依赖）。
- 假设各主流 macOS 终端在应用开启鼠标捕获后会停止把滚动翻译成方向键、转而发送 SGR 鼠标事件（crossterm 标准行为）。
- 假设列表每行高度恒为 1 行（命中测试按行号线性映射）；若渲染存在多行项或分组标题，planning 需在命中测试中处理。

### Outstanding Questions

**Deferred to Planning**

- 滚轮步进 N 的具体值（倾向 3；可做常量便于调）。
- 点击命中测试的精确算法：如何从全局 (col,row) 得到列表可见行，需要列表视口 Rect 与滚动 offset 字段（探查中）。
- "标记区" vs "行主体" 的点击热区如何划分（整行点击=定位光标，是否留出复选列点击=切换标记，或用不同鼠标键）。
- confirm 清单滚动是否也接入滚轮（R5 倾向接入）。
- 是否顺带支持 Menu 状态的点击选项。

### Sources / Research

- `crates/tui/src/event.rs:18-28` — 事件线程当前只 `event::read()` 出 `Event::Key`，丢弃其余（含鼠标）。
- `crates/tui/src/lib.rs:56-67` — `enable_raw_mode` / `EnterAlternateScreen` / `LeaveAlternateScreen` 开关点，无 `EnableMouseCapture`；R1 的成对开关落点。
- `crates/tui/src/lib.rs:81` — 主循环 `SelectResult::Key(KeyEvent)`，鼠标事件需并入此分发。
- `crates/tui/src/lib.rs:299-343` — confirm 的滚动 clamp 与 `PAGE_STEP` 语义，R3/R4 可参照。
- `CONCEPTS.md` / `crates/core/src/models.rs` — `SafetyLevel` 与 `selected = safety != Risky && preselect`，R7 点击标记必须遵守。
- `docs/solutions/design-patterns/render-layer-sort-permutation-indices.md` — 显示序↔存储序置换层，R8 命中测试复用。

---

## Planning Contract

### Key Technical Decisions

- KTD1. **命中测试用事件时重算布局，而非在渲染时缓存几何**。渲染函数签名为 `&App`（不可变），改成 `&mut` 或引入内部可变性都过度侵入。改为在 `handle_mouse` 时用 `terminal` 当前尺寸重跑同一套**纯布局函数**（`chrome::three_row_layout` / `results::split_body`）得到 `list_area`，与上一帧渲染一致。尺寸在渲染与鼠标事件间极少变，`.get()` 兜底吸收任何竞态。
- KTD2. **抽取共享 `chrome::window_start(cursor, visible_height)`**。`rows.rs` 与 `analyzer.rs` 两处逐字重复的视口起始行公式，抽成单一函数，渲染与命中测试共用——这是命中测试落点与实际渲染行对齐的**唯一真源**，杜绝二者漂移。
- KTD3. **鼠标事件走独立 channel**。`EventHandler` 新增 `mouse_rx`，`event.rs` 读循环放行 `Event::Mouse`；主循环两处（`Select` 动画态 / `select!` 静态态）各加一 arm，新增 `SelectResult::Mouse`。不改 `key_rx` 类型，零触碰现有按键消费点。
- KTD4. **一次左键点击 = 定位光标 + 切换标记**（复用 `toggle_selection` / analyzer 内联 marked 逻辑）。避免按行型解析复选框列的脆弱 X 数学；直接交付"点击选中"。下钻仍留键盘 Enter。分隔行点击 no-op。
- KTD5. **滚轮步进复用既有移动语义**。Results/Scanning 调 `move_cursor_up/down` ×N（含分隔行跳过 + clamp）；Analyzer 直接对 `cursor` 加减 N 并 clamp（Live 置 `user_navigated`）。常量 `MOUSE_SCROLL_STEP = 3`。

### 布局与借用注意

- Analyzer 命中/标记须先 clone 出 path（结束对 `tree_root` 的不可变借用）再改 `app.marked`（`app` 的另一字段），避免借用冲突——与既有键盘标记同一手法。
- `AnalyzingLive` 的点击 idx 是**显示序**，经 `size_desc_order` 映回存储索引取 path；`Analyzing` 的 idx 直接是存储序（`sorted=false`）。`cursor` 赋值：Live 存显示序 idx，Analyzing 存存储序 idx——与各自渲染的 cursor 语义一致。

### 时序

U1（共享几何）→ U2（事件通道 + 捕获开关）→ U3（`handle_mouse` 分派 + 滚动 + 点击）→ U4（测试）。U3 依赖 U1/U2。

---

## Implementation Units

### U1. 抽取共享视口几何 `chrome::window_start`

- **Goal**：单一真源的视口起始行公式，供渲染与命中测试共用（KTD2）。
- **Files**：`crates/tui/src/ui/chrome.rs`（新增 `pub fn window_start`）；`crates/tui/src/ui/rows.rs:53-62`、`crates/tui/src/ui/analyzer.rs:102-107`（改为调用它）。
- **Verification**：`cargo build` 通过，两处渲染行为不变（滚动窗口与改前逐位一致）。

### U2. 鼠标捕获开关 + 事件通道

- **Goal**：终端进出成对开关 `EnableMouseCapture`/`DisableMouseCapture`（含 panic hook）；鼠标事件进入主循环（R1, R2）。
- **Files**：
  - `crates/tui/src/lib.rs:19-23`（导入 `EnableMouseCapture, DisableMouseCapture`）、`:48-53`（panic hook 加 `DisableMouseCapture`）、`:56-57`（`EnterAlternateScreen` 后 enable）、`:66-67`（`LeaveAlternateScreen` 前 disable）。
  - `crates/tui/src/event.rs`（`EventHandler` 加 `mouse_rx`；读循环 `match event::read()` 放行 `Event::Mouse`）。
  - `crates/tui/src/lib.rs:80-86`（`SelectResult::Mouse`）、`:119-150` 动画态 `Select` 加 arm、`:237-251` 静态态 `select!` 加 arm、`:152-232` match 加 `Mouse` 分支（重算 `term_area` 后调 `handle_mouse`）。
- **Verification**：`cargo build`；进 TUI 无捕获残留（退出后 shell 鼠标正常）。

### U3. `handle_mouse` 分派：滚动 + 点击

- **Goal**：实现滚轮步进（R3-R5）与点击定位+标记（R6-R8），守卫 `Cleaning`/覆盖层（R9-R10）。
- **Files**：`crates/tui/src/lib.rs`（新增 `handle_mouse` / `mouse_scroll` / `mouse_click` / `hit_row`；常量 `MOUSE_SCROLL_STEP`）；`crates/tui/src/ui/results.rs`（`split_body` 提为 `pub(crate)` 供命中测试复用）。
- **Patterns**：
  - `Cleaning` 态 return；`confirm_delete` 态仅滚轮调 `confirm_scroll`；`show_help`/`filter_active` 忽略。
  - `hit_row`：边框/区域外→None；`visible_row = row - list_area.y - 1`；`idx = window_start(cursor,vh) + visible_row`；`idx < total ? Some : None`。
  - Results/Scanning：`flat_rows.get(idx)`，非 Separator 则 `result_cursor = idx` + `toggle_selection`。
  - Analyzing：`children.get(idx)`；Live：`size_desc_order().get(idx) → children.get(stored)`，置 `user_navigated`。均先 clone path 再改 `marked`。
- **Verification**：`cargo clippy --all-targets` 干净；`verify-tui` 实测滚动固定步进、点击定位+标记。

### U4. 测试

- **Goal**：命中测试与滚动/标记的纯逻辑单测（不需真终端）。
- **Files**：`crates/tui/src/lib.rs`（`#[cfg(test)]`）。
- **Test Scenarios**：
  - `hit_row`：点首个可见行→窗口起点；点边框/footer→None；滚动后（window_start>0）点第 k 可见行→`window_start+k`；点超出数据的空白→None；`total=0`→None；`visible_height=0`→None。
  - `window_start`：cursor 在首屏→0；超出→`cursor+1-vh`；`vh=0`→0。
  - 滚动步进：Results 连滚 clamp 不越界；Analyzer `scroll_cursor` 边界。
- **Verification**：`cargo test -p mc-tui` 全绿。

## Definition of Done

- `cargo build --release` 通过；`cargo clippy --all-targets` 无警告（pedantic 全开）。
- `cargo test` 全绿（含新增命中测试）。
- `verify-tui` 实测：滚轮/触控板每 tick 稳定移动 3 行不飞；点击列表行定位光标并切换标记；点击边框/空白无效；退出后终端鼠标恢复正常。
- 覆盖 Results / Scanning / Analyzing / AnalyzingLive 四态；`Cleaning` 态忽略鼠标。
