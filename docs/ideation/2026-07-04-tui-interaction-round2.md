---
date: 2026-07-04
topic: tui-interaction-round2
focus: 对齐 Analyze 手感的响应性、修复删除后退出与键位诡异（成熟化工作之后暴露的新问题）
mode: repo-grounded
grounding: 编排者亲自在 tmux 逐屏跑 mc（Menu/Uninstall/Analyze/删除确认）+ dua-cli 源码级最佳实践 + 6 帧
supersedes_context: 2026-07-04-tui-ux-maturity.md 的问题多已由 commit 64ce226 落地；本轮针对其后暴露/引入的问题
---

# Ideation: 交互响应性 + 删除安全 + 键位一致（成熟化之后的第二轮）

> 用户诉求（原话）："除了 Analyze 的其他几个命令，体验都要对齐 Analyze，现在点击一下半天没反应；进一步优化 Analyze，键盘交互体验比较奇怪，可以调研最佳实践；删除文件后莫名就退出了。先分析问题、实际体验、再给可行优化。"
> **本轮 grounding 是第一手实测**：编排者跑了 release 二进制，触发 Uninstall 观察冻结、跑 Analyze 观察流式扫描、并**真实复现了一个数据丢失级陷阱**（见下）后安全取消。

## 与上一轮的关系（不重复）

`2026-07-04-tui-ux-maturity.md` 提出的 7 项（标记→删除闭环、统一标记集、keymap 单一事实源、`?` help、扫描中实时排序、翻页跳转、确认路径清单）**大多已在 commit `64ce226` 落地**。本轮三个抱怨恰是那轮工作之后的产物：

| 用户抱怨 | 与上一轮的因果 |
|---|---|
| 删除后莫名退出 | 上一轮打通了 Analyzer 删除闭环（旧 #1），但删除后走 `Cleaning→Done→back_to_menu` 把整棵树拆掉——**闭环实现引入的新退出问题** |
| Analyze 键盘诡异 | 标记闭环 + "自动跟随最大项"光标叠加，产生**误标陷阱**；且 Results 与 Analyzer 键位模型分叉 |
| 其他命令卡顿 | Uninstall 仍走**主线程同步** `list_apps()`，未享受 Analyze 的后台流式（上一轮未覆盖） |

## Grounding Context（第一手实测证据）

**架构现状（当前代码，非旧 doc）:** `AppState` = Menu/Scanning/Results/Cleaning/Done/Analyzing/AnalyzingLive/Sorting。主循环 lib.rs 双分支：动画态用 `crossbeam::Select` 多路复用（键盘+进度+analyze+sort，100ms 超时），静态态先 draw 再纯阻塞 `select!`。`marked: HashSet<PathBuf>` 已是统一标记集。`confirm_delete: Option<Vec<(PathBuf,u64)>>` 覆盖层。keymap.rs 是 help+footer 的单一事实源（但仅驱动"显示"，不驱动"分发"）。

**实测证据（direct）:**

1. **Uninstall 主线程冻结** — `start_command` 对 Uninstall 分支**同步**调用 `AppResolver::list_apps()`（lib.rs:676），而 `list_apps` 对每个 `.app` 调 `calc_app_size()` 全量遍历包体积（app_resolver.rs:87）。**无 Scanning 态、无 spinner**：按 Enter 后菜单僵住直到整份列表算完才跳 Results。实测本机 61 应用/41.5GB 冻结约 0.3–0.5s 且期间界面完全无反馈；含 Xcode 等大包的机器会到数秒。对比 Clean/Purge/Analyze 都是后台线程 + 即时 Scanning/Live 态。

2. **删除后拆树退出** — Analyzer 里 `x`→确认→`start_cleaning`（lib.rs:1010）把 `AppState::Analyzing` 整个替换成 `Cleaning`（**树被 drop**）→ `CleaningDone` 置 `Done`（:836）→ 用户按键 `handle_done_key`→`back_to_menu`（:1040/:464 清空 `scan_result`+`marked`+全部）。即：在磁盘浏览器里删一项，被弹出树、进"完成"页、再回主菜单，**整棵扫描树丢失，要继续清理得重扫**。这就是"删除后莫名退出"。dua 的对照：删除后 `focussed = Main`，焦点回列表继续操作。

3. **误标数据丢失陷阱（实测复现）** — Analyze 扫描进行中，光标因 `!user_navigated` 被钉在显示序 0（跟随当前最大项，lib.rs:885-889）。编排者本想标记看到的 `.net`(284MB)，按 `d` 时扫描已把最大项换成 `~/Library`，于是 `d`（:1234，用 `order[cursor]` 取项）标记了 `~/Library`。确认框弹出："**项目数量:1，预计释放 54.15GB，• /Users/zhaohejie/Library**"。若按 Enter 即把整个 Library 移入废纸篓。**按 Esc 取消后，顶栏仍显示"已标记删除:1 个"——Library 仍被标记，残留不可见**。且确认框显示 54GB 而列表同刻显示 Library 191GB（标记快照与实时树不一致）。

4. **跨视图键位分叉（footer 实测）** — Results footer：`Space 选择 | Enter 确认清理`；Analyzer footer：`d 标记 | x 删除 | Enter/l 进入目录`。**同一个 Enter：Results 里是"执行删除"，Analyzer 里是"进目录"**；标记键 Space vs d；删除键 Enter vs x。肌肉记忆在两视图间互相背叛。且 `d`=标记 与 `Ctrl+d`=翻页共用字母，`d` 单键误触即标记。

5. **Clean/Purge 首帧手感弱于 Analyze** — 二者确实即时进 Scanning 态（后台线程 OK），但动画分支是"先 select 100ms 再 draw"（lib.rs:340-349），首帧最多迟 100ms；且首个 `Found` 到达前 scan.rs 只显示"已扫描 0 个文件 | 准备扫描..."+spinner，相比 Analyze 每条 entry 可见地涌入，观感"呆"。属"对齐 Analyze 流式手感"的次要项。

**dua-cli 最佳实践基线（源码级，`/Users/zhaohejie/workspace/explore/dua-cli`）:**
- **标记(无副作用)与执行(有副作用)解耦**：主视图 `Space`/`x` 只打标记（`Space` 不移光标，`x` 打标并前进），删除必须 `Tab` 进 Marked pane，用 `Ctrl+t`(废纸篓)/`Ctrl+r`(永久删) 触发——毁灭性操作用 Ctrl+ 组合键而非单字符，且并列一个可逆姊妹操作、颜色区分。
- **删除后焦点回主列表**，不退出程序；批量删除逐项重绘 + 状态栏计数。
- **分层退出 + 沉没成本二次确认**：`pending_exit` 字段——有未处理标记或扫描代价高时，第一次 q/Esc 只置 pending 并提示，第二次才真退；无沉没成本则直接退。避免"操作半天一个 q 手滑退整个程序"。
- **后台线程 + channel + `select!` 竞速输入**：扫描中键盘照常实时响应（mc 的 Analyze 已是此模型，Uninstall 尚未）。
- **全局键（`?`/退出/`Tab` 切焦点）在事件分发最外层统一处理**，子视图只定义自己独有的键——多视图一致性的落地方式。

## Topic Axes

1. **启动与感知响应性** — Uninstall 主线程冻结、Clean/Purge 首帧、流式手感对齐 Analyze
2. **删除流程与删除后状态** — 拆树退出、误标陷阱、确认快照一致性、残留标记可见性
3. **键位模型一致性** — 标记/删除/进入 语义跨视图统一、对齐 dua/ncdu 惯例
4. **退出与返回层级语义** — q 全局秒退、Esc 歧义、沉没成本二次确认
5. **可见状态与反馈** — 残留标记、大小快照、上下文键位提示

> 6 帧 × ~8 ≈ 48 原始候选，去重后按"是否直接闭合实测抱怨 × 安全严重性 × dua 对齐 × 可行性"排名，保留 6 幸存者。**#1 是数据丢失级安全项，优先级高于一切响应性/美观项。**

---

## Ranked Ideas（幸存者）

### 1. 删除流程重构：删后留在树内 + 消除误标陷阱 + 确认快照一致 【安全第一】
**闭合抱怨:** #3 删除后莫名退出 + 实测数据丢失陷阱
**Description:** 三合一修复 Analyzer 删除路径：
- **(a) 删后不拆树**：删除完成后从内存树按路径剔除已删节点、光标落到相邻项、**停留在 Analyzing 视图**继续操作，不再走 `Cleaning→Done→back_to_menu`。（对照 dua 删后 `focussed=Main`。）
- **(b) 消除误标**：用户一旦按标记键即视为手动介入——立刻置 `user_navigated=true` 冻结自动跟随，使标记始终作用于"用户当前所见的那一项"而非"此刻最大项"；或更彻底：扫描进行中禁用标记/删除，仅完成后（Analyzing 态）可标记。二选一需团队定。
- **(c) 快照一致 + 残留可见**：确认框大小取标记时快照并与列表显示口径统一；Esc 取消删除**同时清除本次待删标记**（或在顶栏/列表用醒目 `[D]` 保证残留标记始终可见可撤）。
**Axis:** 2 | **Basis:** `direct:` lib.rs:1010 `start_cleaning` 替换 Analyzing 态、:836 CleaningDone→Done、:1040/:464 back_to_menu 清空树；:885-889 auto-follow 钉光标 + :1234 `d` 用 order[cursor]；confirm.rs 汇总快照 vs collect_marked(:1055) 读实时 node.size。编排者实测复现标记 ~/Library。`external:` dua handlers.rs 删后回 Main、Ctrl+t/Ctrl+r 与 Marked pane 解耦。
**Rationale:** 这不只是体验问题——实测证明当前设计能让用户在无意中把 54GB 系统 Library 送进废纸篓。清理工具的删除路径出现"想删 A 却删了 B"是信任崩塌级缺陷，必须先修。删后拆树同时直接消灭"莫名退出"。
**Downsides:** 删后剔除节点要维护树/`nav_path`/`cursor` 一致性（复用现有导航不变式）；"扫描中是否允许标记"是产品判断。
**Confidence:** 95% | **Complexity:** Medium | **Status:** Unexplored

### 2. Uninstall 异步流式化（对齐 Analyze/Clean 的后台线程模型）
**闭合抱怨:** #1 点一下半天没反应（最直接的一处）
**Description:** 把 `AppResolver::list_apps()` 从主线程搬进后台线程，按 Analyze/Clean 同款模式：按 Enter 立刻进一个 Scanning/Live 态显示"扫描应用中… + spinner + 已发现 N 个应用"，逐个 `.app` 解析完就流式追加，`calc_app_size` 的重活不再阻塞事件循环。顺带：应用体积可后台增量算（先出名字/路径，size 随后填），进一步缩短首屏。
**Axis:** 1 | **Basis:** `direct:` lib.rs:671-701 Uninstall 分支同步 `list_apps()` 后直接 `init_results()`，无 Scanning 态；app_resolver.rs:87 `calc_app_size` 每 app 全量 walk；实测菜单僵住 0.3–0.5s（大应用机器更久）无任何反馈。`external:` dua 后台遍历 + select! 竞速；Clean 分支(lib.rs:538) 已是 `thread::spawn` 范本可照抄。
**Rationale:** 这是"其他命令卡顿"里唯一真正的 UI 冻结（Clean/Purge 已异步）。改动集中（照搬 Clean 分支结构），见效最直接。
**Downsides:** 应用列表流式追加时排序/光标要稳定（先占位后填 size 会引起重排，参考 Analyze 的 auto-follow 处理）。
**Confidence:** 90% | **Complexity:** Low-Medium | **Status:** Unexplored

### 3. 跨视图统一键位：Space=标记（不移光标）/ 单一删除键 / Enter 永不删
**闭合抱怨:** #2 键盘诡异
**Description:** 收敛 Results 与 Analyzer 的选择/删除心智为一套（对齐 dua）：
- **标记** 统一为 `Space`（两视图一致，且不移动光标）；保留 `d`/`x` 作为 dua 式"标记并前进"的可选加速键。
- **删除执行** 统一为一个非导航键（`x` 或 `Ctrl+t` 移废纸篓，`Ctrl+r` 永久删），**Results 里不再让 `Enter` 触发删除**——`Enter` 在所有列表里只做"进入/展开"或无害确认。
- Analyzer 与 Results 的"进入目录 / 展开分类"归一到 `Enter`/`l`/`→`，"返回"归一到 `Backspace`/`h`/`←`。
**Axis:** 3 | **Basis:** `direct:` keymap.rs Results(`Space 选择`/`Enter 确认清理`) vs Analyzing(`d 标记`/`x 删除`/`Enter 进目录`)；handlers lib.rs:985(Space)/:999(Enter 删) vs :1129(d)/:1137(x)；实测两 footer 并列可见冲突。`external:` dua help.rs 全键位表：Space 标记不移光标、x 标记并前进、Ctrl+t/Ctrl+r 删除。
**Rationale:** "同一个 Enter 在一个视图删文件、在另一个视图进目录"是键盘诡异的根因，也是误删温床。统一后肌肉记忆跨视图一致，且把毁灭键从 `Enter`（最常按）挪开。
**Downsides:** 改变现有用户既有肌肉记忆（项目尚早，代价可接受）；需与 #1(b) 的标记语义协同定稿。
**Confidence:** 85% | **Complexity:** Low-Medium | **Status:** Unexplored

### 4. dua 式退出契约：分层返回 + 沉没成本二次确认（q 不再全局秒退）
**闭合抱怨:** #3 莫名退出（次要成因）
**Description:** 重定 q/Esc 语义：`Esc`/`Backspace`/`h`/`←` = 返回上一层（子视图→菜单→…），`q` = 退出**当前层**而非永远整程序秒退。引入 dua 的 `pending_exit`：当存在未处理标记、或扫描/树代价高时，第一次退出键只置 pending 并在状态栏提示"再按一次退出"，第二次才真退；无沉没成本则直接退。Done 页的 `q` 也纳入此契约，避免在完成页手滑 q 直接杀进程。
**Axis:** 4 | **Basis:** `direct:` lib.rs:437 全局 `q`→`should_quit`（除 Cleaning 外任意态秒退）；:503 Menu 的 Esc 也直接退；Done 页 :1040 任意 Enter/Esc/Backspace→menu 但 q 仍全局退。`external:` dua eventloop.rs `handle_quit` 分层 + `pending_exit` + `is_costly()` 判定。
**Rationale:** 与 #1 的拆树共同构成"删完就退"的观感；即便删除留在树内，一个全局秒退的 q 仍会在长时间操作后被手滑触发。二次确认只在有沉没成本时收敛，不牺牲空闲场景的干脆退出。
**Downsides:** "沉没成本"判定阈值（多大扫描算 costly）需定；状态栏需有稳定的提示位承载"再按一次"。
**Confidence:** 80% | **Complexity:** Low-Medium | **Status:** Unexplored

### 5. Clean/Purge 首帧即时 + 流式手感对齐 Analyze
**闭合抱怨:** #1 卡顿感（Clean/Purge 部分）
**Description:** 两点微调补齐流式手感：(a) 菜单态处理完 Enter 后**立即 draw 一帧** Scanning（或动画分支进入即先 draw 再 select），消除首帧最多 100ms 的空档；(b) 首个 `Found` 到达前，Scanning 屏就展示"正在扫描 <顶层目录> …"的滚动路径/规则名而非静态"准备扫描..."，让"有事在发生"可见（复用已有的 `ProgressEvent::Scanning`/`RuleProgress`）。
**Axis:** 1 | **Basis:** `direct:` lib.rs:340-349 动画分支 select_timeout 在前、draw 在后；scan.rs render_progress 首个 Found 前恒显 "已扫描 0 个文件 | 准备扫描..."；handle_progress 已收 Scanning/RuleProgress（:710/:730）但首屏未强调。`external:` dua 扫描即显 entries/s 吞吐 + 实时路径。
**Rationale:** Clean/Purge 已异步，差的是"即时可见的活性"。低成本把观感拉到 Analyze 一档。
**Downsides:** 纯观感优化，优先级低于 #1–#4 的安全/退出问题。
**Confidence:** 75% | **Complexity:** Low | **Status:** Unexplored

### 6.（杠杆/复利）四命令统一到 Analyze 级交互内核 + keymap 表驱动分发
**闭合抱怨:** 三个抱怨的共同根因
**Description:** 把 Analyze 已有的"后台流式 + select! 竞速 + 可导航增量态 + 就地删除"抽成所有命令共用的交互内核；并把 keymap.rs 从"仅驱动 help/footer 显示"升级为"同时驱动实际键位分发"（`Action` enum + 每态支持的 Action 子集，主循环查表），使显示的键、帮助的键、真正生效的键三者永不漂移，退出/标记/删除语义天然跨视图一致。#2/#3/#4 都是这套内核的自然属性。
**Axis:** 3+1+4 | **Basis:** `direct:` lib.rs 已有双分支 select! 内核（:233-366）本可复用，但 Uninstall 走同步旁路、Results 走独立 handler；keymap.rs 是显示单一源，而 handle_*_key 各自重新编码键位（显示与分发两套）。`external:` dua `FocusedPane` + 外层统一分发 + 子面板只定义独有键。
**Rationale:** 与其逐命令打补丁，一次把四命令拉到同一交互内核，后续每个命令"响应快、键位一致"成为构造性保证而非反复修。也为 STRATEGY.md"多界面适配"轨道（GUI 复用同引擎/同交互语义）铺路。
**Downsides:** 架构级重构、scope 大，需推倒 Uninstall 旁路与 Results 独立 handler；宜作方向而非本轮快赢——建议 `/ce-brainstorm` 单独定义。#1–#5 都是通往它的增量基石，各自独立成立。
**Confidence:** 80% | **Complexity:** High | **Status:** Unexplored

---

## Rejection Summary

| Idea | 帧 | 拒因 |
|---|---|---|
| 删除撤销倒计时 toast | 类比 | 文件入废纸篓天然可恢复；"残留标记可见 + Esc 清标记"已并入 #1(c) |
| Marked pane 独立面板（dua 式 Tab 切） | 杠杆 | 价值真实，但属 #6 统一内核的一部分；单独做会与现有 confirm 覆盖层重复，先并入 #6 方向 |
| 全量二次模态确认所有删除 | 约束 | lazygit 社区共识"确认键≠触发键"更优；#3 把毁灭键挪离 Enter + #4 沉没成本确认已覆盖，避免每次都加模态拖慢 |
| 鼠标点击选择 | 反转 | 键盘一致性(#3)未闭合前引入鼠标是镀金；opt-in 低优先 |
| Uninstall 加搜索/过滤 | 痛点 | 真实缺口但属上一轮 #3 StatefulList 范畴；本轮先解冻结(#2)这个更痛的点 |
| 进度条亚格精度/主题 | 痛点 | 美观项，与本轮"响应性+安全+一致"三抱怨正交，留待 raise-the-bar |

## 建议实施顺序（含依赖）

```
安全先行:   #1 删除流程重构（删后留树 + 消除误标 + 快照一致）   ← 数据丢失级，最先做
响应性:     #2 Uninstall 异步流式   +   #5 Clean/Purge 首帧即时
一致性:     #3 统一键位（Space 标记 / 删除键非 Enter）   +   #4 退出契约（分层 + 沉没成本二次确认）
方向(brainstorm): #6 四命令统一交互内核 + keymap 表驱动分发
```

- **#1 必须最先**：实测证明当前能误删 54GB 系统目录，且它同时消灭"删除后退出"。若只做一件事，做 #1。
- **#2 是"卡顿"里唯一真冻结**，改动集中（照抄 Clean 后台分支），投入产出最高。
- **#3/#4 一起做**：键位统一与退出契约都围绕"把毁灭性/退出动作从高频键挪开"，协同定稿最省事。
- **#6 是复利方向**：#1–#5 恰是通往它的增量基石，即便不做完全统一也各自成立——建议单独 `/ce-brainstorm`。

## 下一步

三个抱怨已各有对应幸存者，且 #1 是需立即处理的安全项。建议：
- 若要立刻动手：从 **#1** 开始（可直接进 `/ce-plan` 或直接实现）。
- 若要先把"统一交互内核(#6)"想清楚再决定 #1–#5 的边界：走 `/ce-brainstorm`。
