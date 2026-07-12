---
title: "新增引擎消费方前先审视协作式取消契约：旧消费方的用法可能一直掩盖真实语义"
date: 2026-07-12
category: design-patterns
module: "mc-core Engine 与 mc-gui purge 集成"
problem_type: design_pattern
component: background_job
severity: high
applies_when:
  - "给已有核心引擎/服务接入新的 UI 消费方，且引擎存在协作式取消等中断路径"
  - "旧消费方一直只信流式事件、忽略或丢弃函数返回值，从未真正暴露返回值在中断路径下的契约"
  - "新消费方把引擎返回值当作权威结果，直接写入持久状态槽（如覆盖 last_result）"
  - "同一操作可能被重复触发，多个异步调用先后 resolve，需要判代次而非 last-writer-wins"
symptoms:
  - "取消扫描后 Results 中出现 0 B 的已预选可删除项，释放量被低报，违反「取消不残留」预期"
  - "被取消的慢速旧一轮扫描异步 resolve 时，用其部分结果覆盖新一轮扫描已写入的结果状态槽"
root_cause: missing_validation
resolution_type: code_fix
related_components:
  - "mc-core scanner/engine"
  - "mc-gui Tauri commands"
  - "mc-gui frontend store"
tags:
  - "tauri"
  - "cancellation-contract"
  - "engine-consumer"
  - "partial-result"
  - "race-condition"
  - "gui"
---

# 给核心引擎接新 UI 消费方时，必须审视取消/返回值契约

## Context

macCleaner 的 `mc-core` 引擎（`crates/core/src/engine.rs`）是 CLI/TUI/GUI 三端共享的薄 facade。扫描类调用（`Engine::scan_purge`）内部走**协作式取消**：调用方持有一个可从外部置位的取消标志，扫描循环/并行池周期性查询它，一旦发现被取消就提前退出——但函数签名不变，最终仍然 `Ok(ScanResult)` 返回，只是结果是**未测完的部分体积**。

具体到 Purge 的目录大小并行计算池（`crates/core/src/scanner.rs:679-681`）：

```rust
// crates/core/src/scanner.rs:679-681（scan_purge_dir 内的并行 map）
if reporter.is_cancelled() {
    return (path.clone(), 0, meta.clone());
}
let size = dir_size(path, reporter);
```

被取消后未测完的目录被记成 **0 体积**，随后照常汇入 `category_map` 参与 `build_scan_result`，整个 `Engine::scan_purge` 调用**仍然 `Ok(...)` 返回**——取消从未在类型层面体现为错误。

这个契约在仓库里已经存在了很久，却从未暴露过问题，原因是**唯一的旧消费方从不看返回值**。TUI 的调用点（`crates/tui/src/command.rs:90` 与 `crates/tui/src/command.rs:116`，Clean 与 Purge 各一处）是同一形状：

```rust
// crates/tui/src/command.rs:115-120
match Engine::scan_purge(&path, &reporter) {
    Ok(_result) => {}                                   // 返回值被整体丢弃
    Err(e) => {
        reporter.on_event(ProgressEvent::Error(e.to_string()));
    }
}
```

TUI 只信 `TuiReporter` 一路流出的 `ProgressEvent`，而 `TuiReporter` 在 `cancelled` 置位后会**直接丢弃后续事件**（`reporter.rs` 有专门测试覆盖这条丢弃路径）。所以「取消后 `Ok(partial)` 里混着 0 体积项」这件事，十几个版本里始终被「没人读这个返回值」悄悄掩盖——它是一个**潜伏（latent）但一直存在**的契约缺口，不是这次改动引入的新 bug。

PR #49（分支 `feat/gui-move7-purge-entry`）把 GUI（Tauri）接入 `Engine::scan_purge` 作为 Purge 页面的后端，是这个引擎第一次有消费方**真正使用返回值**：`crates/gui/src/commands/purge.rs` 的 `scan_purge` 命令把 `Engine::scan_purge` 的返回结果存进 `last_purge` 状态槽，后续 `purge` 命令删除时从这个槽里按路径取项。多代理评审在这次接入中揪出两类污染：

1. **取消后 0 B 预选项可删**——未测完目录被记成 0 体积，若其 `safety != Risky` 仍会被预选中；GUI 一旦把这个 `Ok(partial)` 当权威结果写入 `last_purge`，用户可以对着一批「体积显示为 0、其实还没扫完」的项发起删除。
2. **慢速被取消的旧扫描覆盖新扫描结果槽**——用户中途取消并重新发起了一次新扫描，若旧扫描的取消线程晚于新扫描完成并回写 `last_purge`，新扫描的正确结果会被旧扫描的部分结果覆盖。

两个问题的根因是同一个：**GUI 是这个「Ok(partial) 语义」的第一个真实消费方，而这个语义从设计之初就没有被当作「需要在调用方过滤」的契约点来对待。**

## Guidance

给已有核心/服务层接一个新调用方时，不能只看「新调用方需要什么数据」，还要反过来审视：**核心层的取消/错误/部分结果语义，此前是靠哪个旧消费方的『恰好不看』才没暴露问题？新消费方会不会正好触碰到那条从未被验证过的路径？**

具体到本例，`crates/gui/src/commands/purge.rs` 的修复（PR #49 的评审修复提交）在 `spawn_blocking` 闭包内、`Engine::scan_purge` 返回之后、写状态槽之前，补一道取消检查：

```rust
// crates/gui/src/commands/purge.rs（scan_purge 命令，修复后）
let result = tauri::async_runtime::spawn_blocking(move || {
    let reporter = TauriReporter::new(on_event, cancelled.clone());
    let result = Engine::scan_purge(Path::new(&path), &reporter);
    // 取消的扫描不得成为权威结果（评审 R2）：核心在取消时把未测完目录记 0 体积且仍
    // Ok(partial)，TUI 丢弃返回值无此暴露；GUI 若照存 last_purge 会出现「取消后 0 B
    // 预选项可删」与「慢速被取消的旧扫描覆盖新结果槽」两类污染——此处统一拒绝。
    if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("扫描已取消".to_string());
    }
    result.map_err(|e| format!("扫描失败: {e}"))
})
.await
.map_err(|e| format!("扫描线程异常: {e}"))??;
*last_purge.lock().map_err(|_| "状态锁毒化".to_string())? = Some(result.clone());
```

关键点：检查放在 `Engine::scan_purge` **返回之后**、状态槽写入**之前**——不是改核心引擎的签名（改了会牵动 CLI/TUI 两个既有消费方及其测试），而是在新消费方这一侧补上「审查取消 flag 再决定是否采信返回值」这一步。这同时解决了两类污染：拒绝写槽消灭了「0 B 预选项可删」；而「取消的结果不写槽」意味着旧扫描即便晚完成，也不会覆盖新扫描已经写入的正确结果（新扫描只要没被取消，其写入不受旧扫描的失败分支影响）。

前端（`crates/gui/frontend/src/routes/Purge.svelte:146-173`）对应把这个 `Err` 分支收敛成统一的「清空回 idle」：

```javascript
// crates/gui/frontend/src/routes/Purge.svelte:146-173（startScan 的 catch/收尾）
} catch (err) {
  // 取消也走这里；reject 后部分项会被清空回 idle，故无论已流入多少项都要留横幅说明原因。
  if (error === null) error = String(err);
}
...
if (result) {
  // 以 resolved ScanResult 为权威终值（消除流式/终态漂移，同 Clean KTD5）。
  items = result.categories.flatMap(...);
  setPhase("results");
} else {
  // 扫描被取消或命令失败（评审 R2）：后端此时不写 last_purge，若保留流式部分项
  // 会形成「可见但删除必然落空」的假结果——清空回 idle，错误横幅仍呈现原因。
  resetToIdle();
}
```

`resetToIdle()`（同文件 73-79 行）清空 `items`/`index`/`buffer`/`skipped`/`lastReport`，回到 `idle`——流式阶段已经渲染出来的部分 `Found` 项不会残留成一个「看着能删、实际删不掉」的假列表。e2e 用例 `crates/gui/frontend/e2e/purge.spec.ts:152-171`（AE6）把这条路径钉成回归测试：模拟扫描流入部分项后点击取消，断言 `cancel_scan` 被调用、页面回到「开始扫描」按钮可见的 idle 态、且流入过的项（`/Node\.js/` 行）不再出现。

## Why This Matters

- **『旧消费方不看返回值』不等于『返回值语义是对的』。** 只要还有一个消费方在用这条路径，契约缺口就有暴露的可能；缺口能潜伏多年只是因为没人真正读过它，不代表可以放心复制这个语义给下一个消费方。
- **新增消费方是审视旧契约的免费触发点。** 不需要专门排期做契约审计——每次接入新 UI/新调用方，天然要过一遍「这个函数在异常/取消路径下到底返回什么」，顺手把潜伏问题挖出来比事后从 bug 报告倒推便宜得多。
- **修复点选在调用方而非引擎，是有意的最小改动面。** 改 `Engine::scan_purge` 的签名（比如取消时返回 `Err`）会同时影响 CLI 的 `--dry-run`/正常路径语义和 TUI 现有测试，波及面远大于「新消费方在读到返回值后自己加一道判断」。当旧契约已经跑了很久、且只有新消费方真正依赖返回值时，优先在新消费方这一层收敛，而不是牵动核心引擎的公共签名。
- **两类污染表面不同、根因相同，修一处即可双解。** 「0 B 预选项可删」是数据正确性问题，「旧扫描覆盖新结果」是并发/时序问题，看似要分别处理，但两者的共同前提都是「取消后的 `Ok(partial)` 被当权威结果采信」——掐断这个采信点，两个症状同时消失，不必为每个症状单独打补丁。

## When to Apply

触发信号：

- 正在给一个多端共享的核心层（引擎/服务/SDK）接入一个**新的 UI 或调用方**，而这个核心层已经有一到多个**长期存在**的旧消费方；
- 核心层存在**协作式取消**、**尽力而为（best-effort）降级**或类似「异常路径不改变返回类型、只改变返回内容」的设计（例如取消后仍 `Ok(partial)`、失败降级后仍 `Ok(default)`）；
- 你能确认旧消费方对这条路径的处理方式是**忽略返回值 / 只信另一条并行的事件流 / 从不触发这条异常路径**——这正是「契约缺口为什么没暴露」的证据，也是新消费方最可能踩中的地方；
- 新消费方打算把返回值**持久化为某个权威状态**（写数据库、写状态槽、驱动后续不可逆操作如删除），而不只是临时展示——一旦持久化，部分/降级结果造成的污染就会脱离当次调用的生命周期，影响后续交互。

不适用：新消费方与旧消费方对返回值的消费方式完全一致（比如都是「只展示，不持久化，出错就整体丢弃」），此时旧契约已经被等价地验证过，不需要重新审视。

## Examples

**审视清单**（把这几个问题问一遍，通常几分钟就能定位契约缺口）：

1. 这个函数在「被取消」「部分失败」「降级」路径下，返回的是 `Err` 还是内容异常的 `Ok`？—— 读源码，不要读文档/注释猜测，注释可能滞后。
2. 现有消费方是否真的在检查这个返回值？—— 全仓 `rg` 一下调用点，看 `match` 分支是不是把 `Ok(_)` 整个丢弃。
3. 如果丢弃，它是靠另一条独立通道（事件流、回调）拿到真实状态的吗？那条通道在取消/异常时是否也做了对应的丢弃处理（如本例 `TuiReporter` 的丢弃事件）？
4. 新消费方打算怎么用这个返回值——只展示一次，还是写入会被后续操作读取的持久状态？后者是审视的高优先级信号。
5. 如果发现缺口，优先在新消费方一侧补检查（如本例的 `cancelled.load(...)` 判断），除非所有消费方都需要这个修复，才考虑改核心层签名。

**已知残余（不在本次修复范围内，留痕供下次审视复用）**：`crates/core/src/scanner.rs:554-556`（`scan_with_rules`，服务于 `Engine::scan_clean`）有完全同构的取消分支——被取消的规则/文件同样可能以 0 体积汇入结果、整体仍 `Ok(...)`。目前 Clean 路径的两个消费方（TUI 丢弃返回值；GUI 的 `scan_clean` 命令若走同一模式）经核查（GUI `scan_clean` 命令写 `last_scan` 前无取消检查分支）尚未复用本次的取消检查。这是 PR #49 记录的 Known Residual，下次改动 GUI 的 Clean 命令或新增消费方时，应作为本条学习的第一个待验证对象。

## Related

- **PR #49**（分支 `feat/gui-move7-purge-entry`，评审修复随该 PR 提交）：GUI 接入 `Engine::scan_purge` 及多代理评审 R2 发现与修复；PR 描述中的 "Known Residuals" 记录了 `scan_clean` 的同型缺口。
- **源码**：`crates/core/src/scanner.rs:679-681`（Purge 并行池取消分支）、`:554-556`（Clean 侧同构分支，已知残余）；`crates/tui/src/command.rs:90` 与 `crates/tui/src/command.rs:116`（TUI 丢弃返回值的两处）；`crates/gui/src/commands/purge.rs`（`scan_purge` 命令的取消检查修复）；`crates/gui/frontend/src/routes/Purge.svelte:73-79`（`resetToIdle`）与 `crates/gui/frontend/src/routes/Purge.svelte:146-173`（reject 收敛）；`crates/gui/frontend/e2e/purge.spec.ts:152-171`（AE6 回归用例）。
- [[analyze-unknown-path-deletion-fail-closed]]（`docs/solutions/security-issues/analyze-unknown-path-deletion-fail-closed.md`）：概念姊妹篇——同属「给 mc-core 接新消费方时，宽松/沉默语义不得被当作安全默认，需 fail-closed」家族（该文是分类证据面，本文是取消返回值面）。
- CLAUDE.md 关于 `ProgressReporter`「取消是协作式」与 TuiReporter 丢弃事件的现有约定，是本例契约缺口得以潜伏的直接原因，值得在改动 `Engine`/`ProgressReporter` 相关代码前重读。
