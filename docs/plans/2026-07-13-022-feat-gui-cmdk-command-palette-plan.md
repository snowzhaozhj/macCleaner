---
artifact_contract: ce-unified-plan/v1
artifact_readiness: implementation-ready
execution: code
plan_type: feat
title: "feat: GUI Cmd+K 命令面板加速器"
created: 2026-07-13
product_contract_source: ce-plan-bootstrap
origin: docs/ideation/2026-07-07-gui-redesign-ideation.md（Ranked Idea #7，move7 未落地部分）
depth: standard
---

# feat: GUI Cmd+K 命令面板加速器

## Summary

给 GUI 补上 `Cmd+K` 命令面板——GUI 重设计 7-move 路线里 move7 唯一未落地的部分（purge/uninstall 入口与四 tab 可见导航已合并）。面板是 Raycast/Linear 式的**开发者加速器**：一个键盘唤起、模糊匹配、可键盘导航的浮层，跨 clean/purge/uninstall/analyze 快速跳转，并暴露少量与路由内部状态无关的全局动作。

**定位铁律（来自 ideation #7 护栏）**：命令面板对普通用户不可见，**只能是加速器、不能是唯一入口**——现有四 tab 可见导航保持不动。**半成品面板比没有更糟**：模糊匹配、焦点陷阱、与全局 modal（`ConfirmDelete`）视觉一致，三者缺一不上。不加侧边栏。

**Product Contract preservation:** 无上游 brainstorm，本计划自 bootstrap；范围即 ideation #7 的命令面板子项，不引入新产品行为。

---

## Problem Frame

`mc-core` 的四条能力（clean/purge/uninstall/analyze）已全部有可见 tab 入口，但开发者（主要用户）在多功能间跳转只能鼠标点 tab。竞品（Raycast/Linear/CleanMyMac 的部分形态）用 `Cmd+K` 命令面板作专家加速器，是 GUI 重设计 ideation 明确排期的 move7 收尾项，且规则/路由数据已齐备，属**纯呈现层**新增，无需新后端 IPC。

不做它，7-move 重设计始终差一块收尾；做半成品（无焦点陷阱 / 无模糊匹配 / 与现有 modal 割裂）则违反 ideation 硬护栏，反而降低信任。

### Scope Boundaries

**In scope**
- 一个 `CommandPalette.svelte` 浮层组件：backdrop + 居中卡片、搜索输入、过滤列表、键盘导航（↑/↓/Enter/Esc）、点外关闭、焦点陷阱。
- 一个纯函数模糊匹配器（子序列匹配 + 排序），可 vitest 单测。
- 命令注册：4 个导航命令（清理/开发清理/卸载/分析）+ 少量全局动作（打开废纸篓、重新检查磁盘权限）。
- `App.svelte` 接线：全局 `Cmd+K` / `Ctrl+K` 监听、面板开合状态、执行命令后关闭。

**Deferred to Follow-Up Work**
- **路由内动作命令**（"开始清理扫描""执行删除"等）：依赖各路由内部 `$state`，需要一套命令注册表让路由动态注册/注销命令。本轮先做导航 + 全局动作，命令模型预留 `run: () => void` 以便后续扩展。
- 命令**最近使用**排序 / 持久化历史。
- 命令**分组标题**（"跳转""动作"分节）——命令数很少时不必要，超过 ~8 条再引入。

**Outside this product's identity**
- 把 Cmd+K 做成 purge/uninstall 的唯一入口（违反 ideation #7 护栏——可见导航必须承载）。
- 侧边栏导航（ideation 明确否决）。

---

## Requirements

- **R1** 全局 `Cmd+K`（macOS）/ `Ctrl+K` 在 `boot === "ready"` 时唤起面板；再次按下或 `Esc` 关闭。onboarding/checking 态不唤起。
- **R2** 输入框自动聚焦；输入为空时展示全部命令；输入时按模糊匹配实时过滤并按匹配度排序。
- **R3** `↑`/`↓` 在结果间移动高亮（首尾环绕）；`Enter` 执行当前高亮命令；点击某项亦执行。
- **R4** 执行导航命令切换到对应 tab；执行全局动作调用对应 IPC；任一执行后面板关闭并把焦点还给触发前的元素（无焦点陷阱残留）。
- **R5** 点击 backdrop 空白区关闭；面板打开时 `Tab` 键焦点不逃出面板（焦点陷阱）。
- **R6** 视觉与 `ConfirmDelete` 全局 modal 一致（同 backdrop 半透明、`--surface-overlay` 卡片、`--radius`、阴影），但边框用中性 `--border-subtle`/`--accent`，**不用 `--state-danger`**（红只跟随 Risky/不可逆语义，R18）。
- **R7** 四 tab 可见导航保持不动——面板是加速器不是替代。
- **R8** 无匹配时展示"无匹配命令"空态，不报错、不留空白焦点陷阱。

---

## Key Technical Decisions

- **KTD1｜面板状态与命令清单归属 `App.svelte`**：`tab` 的 `$state` 本就在 `App.svelte`，导航命令直接写 `tab = ...` 最短路径；面板开合状态与全局键盘监听同处一层。路由内动作因需路由内部状态而**本轮不接**（见 Scope Deferred）。
- **KTD2｜模糊匹配用零依赖子序列算法**：不引第三方 fuzzy 库（项目 devDependencies 极简，见 `package.json`）。子序列匹配 + 连续/词首加权排序，纯函数放 `src/lib/palette.ts`，独立 vitest。
- **KTD3｜命令模型 `{ id, title, keywords, run }`**：`run: () => void` 让导航与全局动作同构，也为后续路由动作命令预留扩展点。`keywords` 支持中英/拼音别名命中（如 "clean"/"qingli" 命中"清理"）。
- **KTD4｜复用 `ConfirmDelete` 的 modal 骨架而非抽象共享组件**：两者交互差异大（token gate vs 搜索列表），过早抽象得不偿失；只复用 CSS token 与 backdrop/`role="dialog"`/`aria-modal` 模式，保证视觉一致（R6）。
- **KTD5｜键盘监听挂 `window` + 捕获 `metaKey`/`ctrlKey`**：`Cmd+K` 需全局生效，挂 `window` `keydown`；`$effect` 内 `addEventListener`/`removeEventListener` 成对，防泄漏。执行 `preventDefault` 防浏览器默认（devtools 无 Cmd+K 冲突，但 `Ctrl+K` 在某些 webview 是地址栏聚焦，需拦）。

---

## High-Level Technical Design

```
App.svelte (owns: boot, tab, paletteOpen)
  │
  ├── window keydown (Cmd/Ctrl+K) ──► toggle paletteOpen   [仅 boot==="ready"]
  │
  └── {#if paletteOpen}
        └── CommandPalette.svelte
              props: { commands: Command[], onClose }
              ├── <input> (autofocus) ──► query
              ├── filtered = fuzzyFilter(commands, query)   ← src/lib/palette.ts (纯函数)
              ├── ↑/↓ 移动 selectedIndex（环绕）
              ├── Enter / click ──► cmd.run(); onClose()
              ├── Esc / backdrop click ──► onClose()
              └── 焦点陷阱 + 关闭时 restore focus

Command 模型（App.svelte 内构造）:
  导航:  { id:"nav.clean",   title:"清理",     keywords:["clean","qingli"],    run:()=>tab="clean" }
         { id:"nav.purge",   title:"开发清理", keywords:["purge","dev"],        run:()=>tab="purge" }
         { id:"nav.uninstall",title:"卸载",    keywords:["uninstall","xiezai"], run:()=>tab="uninstall" }
         { id:"nav.analyze", title:"分析",     keywords:["analyze","fenxi"],    run:()=>tab="analyze" }
  全局动作: { id:"act.trash", title:"打开废纸篓", run:()=>openTrash() }
           { id:"act.fda",   title:"重新检查磁盘访问权限", run:()=>openFdaSettings() }
```

模糊匹配（`fuzzyFilter`）：对每条命令的 `title + keywords` 做子序列匹配；命中则计分（连续命中、词首命中加权），未命中剔除；按分降序、同分按原序稳定排序。空 query 返回全部（原序）。

---

## Implementation Units

### U1. 模糊匹配器（纯函数 + 单测）

**Goal:** 提供零依赖、可测试的命令过滤/排序函数，作为面板的数据层。

**Requirements:** R2、R8、KTD2、KTD3

**Dependencies:** 无

**Files:**
- `crates/gui/frontend/src/lib/palette.ts`（新建：`Command` 类型、`fuzzyFilter(commands, query)`、内部 `scoreMatch`）
- `crates/gui/frontend/src/lib/palette.test.ts`（新建）

**Approach:**
- `Command = { id: string; title: string; keywords?: string[]; run: () => void }`。
- `fuzzyFilter(commands, query)`：query 为空 → 原序返回全部；否则对 `title` 与每个 keyword 做小写子序列匹配，取最高分；分 > 0 保留；`Array.prototype.sort` 稳定，同分保持原序。
- `scoreMatch(haystack, needle)`：子序列命中基础分，连续命中与 index 0 词首命中加权；无命中返回 0。

**Patterns to follow:** `src/lib/confirm.ts` + `src/lib/confirm.test.ts`（同类纯函数 + vitest 结构）、`src/lib/categories.ts`。

**Test scenarios:**
- 空 query → 返回全部命令、顺序不变。
- `"cl"` → 命中 `clean`（title/keyword 子序列），排在含 `cl` 子序列的其他项之前（按分）。
- 大小写无关：`"CLEAN"` 与 `"clean"` 结果一致。
- keyword 命中：`"qingli"` 命中"清理"（title 是中文，靠 keyword）。
- 词首/连续加权：`"clean"` 对 `title:"清理" keywords:["clean"]` 的分高于仅零散子序列命中的项。
- 无匹配：`"zzzz"` → 返回空数组。
- 稳定性：两条同分命令保持输入相对顺序。

**Verification:** `pnpm test` 中 `palette.test.ts` 全绿。

**Execution note:** 先写失败测试再实现（纯函数、契约清晰，适合 test-first）。

---

### U2. CommandPalette.svelte 浮层组件

**Goal:** 完整的命令面板 UI 与交互：唤起后可搜索、键盘导航、执行、关闭，视觉与全局 modal 一致，带焦点陷阱。

**Requirements:** R2、R3、R4、R5、R6、R8

**Dependencies:** U1

**Files:**
- `crates/gui/frontend/src/lib/CommandPalette.svelte`（新建）

**Approach:**
- Props: `{ commands: Command[]; onClose: () => void }`。
- 本地 `$state`: `query`、`selectedIndex`；`filtered = $derived(fuzzyFilter(commands, query))`。
- 布局复用 `ConfirmDelete` 骨架：`.backdrop`（点 `currentTarget` 关闭）+ `.modal`（`role="dialog"` `aria-modal="true"`）；顶部搜索 `<input autofocus>`；下方结果 `<ul>`（每项 `role="option"`，高亮 `aria-selected`）；空态一行"无匹配命令"。
- 键盘：组件内 `onkeydown`——`ArrowDown/Up` 改 `selectedIndex`（`(i+n)%n` 环绕）、`Enter` 执行 `filtered[selectedIndex].run()` 后 `onClose()`、`Esc` `onClose()`。query 变化时 `selectedIndex` 重置为 0。
- **焦点陷阱**：`Tab`/`Shift+Tab` 在面板内循环；组件挂载时记录 `document.activeElement`，`onClose` 时（由 App 卸载后）焦点还原——还原逻辑放 App 层（见 U3），组件只负责挂载即聚焦 input。
- 视觉：边框 `--border-subtle`（hover/active 用 `--accent`），**禁 `--state-danger`**（R6/R18）；同 `--surface-overlay`/`--radius`/阴影。

**Patterns to follow:** `src/lib/ConfirmDelete.svelte`（backdrop、modal、`role`/`aria`、点外关闭、CSS token）。

**Technical design（directional，非实现规范）:**
```
<div class="backdrop" onclick={closeIfBackdrop} role="presentation">
  <div class="modal" role="dialog" aria-modal="true" onkeydown={handleKeys}>
    <input bind:value={query} autofocus aria-label="命令面板搜索" />
    {#if filtered.length}
      <ul role="listbox">
        {#each filtered as cmd, i (cmd.id)}
          <li role="option" aria-selected={i===selectedIndex}
              class:active={i===selectedIndex}
              onclick={() => run(cmd)}>{cmd.title}</li>
        {/each}
      </ul>
    {:else}
      <p class="empty">无匹配命令</p>
    {/if}
  </div>
</div>
```

**Test scenarios:**（交互态走 e2e，见 U3 的 spec；本单元的组件级断言并入 e2e）
- 渲染全部命令、input 获焦（e2e）。
- 输入过滤 + 高亮首项（e2e）。
- ↑/↓ 环绕、Enter 执行、Esc/点外关闭（e2e）。
- 空态"无匹配命令"（e2e）。

**Verification:** 组件通过 svelte-check（`pnpm check`）无类型/无障碍告警；交互由 U3 e2e 覆盖。

---

### U3. 接线 App.svelte + e2e

**Goal:** 全局键盘唤起、命令清单构造、执行后关闭并还原焦点；e2e 覆盖完整交互链；保持四 tab 可见导航不变。

**Requirements:** R1、R3、R4、R5、R7

**Dependencies:** U1、U2

**Files:**
- `crates/gui/frontend/src/App.svelte`（修改：新增 `paletteOpen` state、`window` keydown 监听、`commands` 构造、`<CommandPalette>` 渲染、关闭时焦点还原）
- `crates/gui/frontend/e2e/command-palette.spec.ts`（新建）
- `crates/gui/frontend/e2e/support/tauri-mock.ts`（按需扩展：确保 `open_trash`/`open_fda_settings` 有 mock，避免全局动作执行时 e2e 报错）

**Approach:**
- `$effect` 内 `window.addEventListener("keydown", onGlobalKey)`，卸载时移除；`onGlobalKey` 命中 `(e.metaKey||e.ctrlKey) && e.key==="k"` 且 `boot==="ready"` 时 `e.preventDefault()`、`paletteOpen = !paletteOpen`。
- 构造 `commands: Command[]`（4 导航 + 2 全局动作，见 HTD）。导航命令闭包写 `tab`；全局动作调用 `openTrash()`/`openFdaSettings()`（已在 `ipc.ts` 导出）。
- 打开面板前记录 `document.activeElement`；`onClose` 置 `paletteOpen=false` 并 `previouslyFocused?.focus()`（R4 焦点还原）。
- 四 tab `<nav>` 原样保留（R7）。

**Patterns to follow:** `App.svelte` 现有 `$state`/`void runFdaCheck()` 风格；e2e 参照 `e2e/clean.spec.ts` + `e2e/support/tauri-mock.ts`、`e2e/support/fixtures.ts`。

**Test scenarios（e2e，command-palette.spec.ts）:**
- `Cmd+K` 在 ready 态打开面板、input 获焦；再次 `Cmd+K` 或 `Esc` 关闭。
- onboarding/checking 态按 `Cmd+K` 不打开面板（R1 边界）。
- 输入"卸载"/"uninstall" 过滤到卸载命令；`Enter` 后切到卸载 tab、面板关闭（R4）。
- ↑/↓ 移动高亮并环绕；点击某项执行（R3）。
- 点 backdrop 空白关闭（R5）。
- 执行"打开废纸篓"命令调用 mock 的 `open_trash`（断言 mock 被调用）。
- 四 tab 导航仍可见可点（R7 回归）。

**Verification:** `pnpm check` 无告警；`pnpm test` 全绿；`pnpm e2e`（沙箱下 `PW_NO_WEBSERVER=1` + 手动 `pnpm dev`，见 Risks）`command-palette.spec.ts` 全绿；`cargo build -p mc-gui` 通过（前端仅纯前端改动，后端不变）。

**Execution note:** 全局键盘监听易泄漏——务必成对 add/remove；`Ctrl+K` 拦截需 `preventDefault` 防 webview 默认行为。

---

## Risks & Dependencies

- **焦点陷阱 / 焦点还原是 slop 高发区**：漏还原焦点、Tab 逃逸都属"半成品面板比没有更糟"的护栏红线。U2/U3 明确覆盖，e2e 断言关闭后 tab 可再获焦。
- **e2e webServer 沙箱超时**（已知）：Playwright 自启 vite 在本机沙箱超时。运行 e2e 时用 `PW_NO_WEBSERVER=1` 并手动 `pnpm dev` 复用已起的 dev server（参见项目记忆 `gui-e2e-sandbox-webserver-workaround`）。ce-test-browser / 验证阶段照此执行。
- **`Ctrl+K` 与 webview 默认冲突**：部分 webview `Ctrl+K` 聚焦地址栏；`preventDefault` 拦截。macOS 主路径是 `Cmd+K`，风险低。
- **命令数少导致面板显薄**：本轮 6 条命令。这是 MVP 的完整形态（机制完整 ≠ 命令穷尽），路由动作命令后续扩展；不因条目少而砍焦点陷阱/模糊匹配。

---

## Definition of Done

- U1–U3 全部落地，`palette.test.ts` 与 `command-palette.spec.ts` 全绿。
- `pnpm check`（svelte-check）无类型/无障碍告警；`pnpm test` 全绿；`pnpm e2e` 命令面板 spec 全绿。
- `cargo build` workspace 通过。
- 四 tab 可见导航行为不回归（R7 e2e 断言）。
- 命令面板视觉与 `ConfirmDelete` 一致、边框中性无红（R6 目测 + 代码审查）。

## Verification Contract

- **单测**：`cd crates/gui/frontend && pnpm test` → `palette.test.ts` 通过。
- **类型/无障碍**：`pnpm check` 零告警。
- **e2e**：`PW_NO_WEBSERVER=1 pnpm e2e`（配合手动 dev）→ `command-palette.spec.ts` 通过 + 既有 spec 无回归。
- **构建**：`cargo build` 通过；`pnpm build` 通过。

## Sources & Research

- `docs/ideation/2026-07-07-gui-redesign-ideation.md` — Ranked Idea #7（Cmd+K 加速器护栏：不可见加速器/可见导航承载/半成品比没有更糟/别加侧边栏），置信 85%。
- 现有代码：`crates/gui/frontend/src/App.svelte`（tab 状态与导航）、`src/lib/ConfirmDelete.svelte`（全局 modal 骨架与 token）、`src/lib/ipc.ts`（`openTrash`/`openFdaSettings` 已导出）、`src/lib/confirm.test.ts`（纯函数单测范式）、`e2e/support/tauri-mock.ts`（e2e mock 范式）。
