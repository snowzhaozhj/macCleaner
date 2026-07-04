---
title: "增量构建树的实时排序：显示层索引置换，而非原地排序"
date: 2026-07-04
category: design-patterns
module: mc-tui
problem_type: design_pattern
component: rails_view
severity: medium
applies_when:
  - "增量/流式构建一棵树并同时要求排序展示"
  - "构建器或导航状态按存储位置索引 children 向量"
  - "需要实时重排列表但不能让存量位置索引失效"
  - "光标要在实时更新中自动跟随某个移动的极值（如最大项）"
tags:
  - tui
  - ratatui
  - incremental-tree
  - render-layer-sort
  - permutation-indices
  - display-vs-storage-order
  - follow-largest
  - cursor-navigation
---

# 增量构建树的实时排序：显示层索引置换，而非原地排序

> 注：`component: rails_view` 是 schema 枚举里对"视图/渲染层"最接近的取值；本模式的实际载体是 Rust `ratatui` 的渲染层（`mc-tui` crate），与 Rails 无关。

## Context

在 macCleaner 的 `mc-tui` crate 里，磁盘扫描是**流式**的：jwalk 以 DFS 顺序不断吐出目录项事件，`IncrementalTreeBuilder` 边扫边把它们拼进一棵 `DirNode` 树。产品目标是——在**扫描仍在进行时**，就把当前节点的子项按体积从大到小展示，并让光标自动跟住最大的那个（"live sort + follow largest"），这样用户第一眼就能看到"谁在吃空间"。

难点在于这棵树有**多个独立的索引消费者**，它们全都假设 `children` 是**发现顺序（append-only）**：

- 构建器用 `depth_stack` 保存"当前 DFS 路径上每一层的子索引"，靠它把新项 append 到正确的父节点；
- 用户导航用 `nav_path`（一串子索引，定位当前进入的节点）+ `cursor`（当前高亮行）；
- 这些索引都是**增量产生**的，指向 `children` 里的固定位置。

一旦你为了展示而 `children.sort_by_key(...)` **原地排序**，所有这些存量索引同时失效：`depth_stack` 会把后续项挂到错误的父节点上，`nav_path` 会跳进错误的子树，`cursor` 会指向错误的行。换句话说，**展示顺序和构建/导航顺序有天然冲突**，而它们又必须共用同一个 `Vec<DirNode>`。

## Guidance

**不要为了改变展示顺序而改动底层结构。把"顺序"下沉到展示层，用一个索引置换（permutation）表达，而底层 `children` 永远保持发现顺序。**

具体三条规则：

1. **提供一个纯函数，产出稳定的展示顺序置换，绝不 mutate 树。**

   ```rust
   /// 返回 `children` 的体积降序置换（0..len 的一个排列），不改动树本身。
   /// 用于展示层：把"渲染行号"映射回"存储索引"。
   pub(crate) fn size_desc_order(children: &[DirNode]) -> Vec<usize> {
       let mut order: Vec<usize> = (0..children.len()).collect();
       // sort_by_key 是稳定排序：体积相等时保持发现顺序，视图不会抖动。
       order.sort_by_key(|&i| std::cmp::Reverse(children[i].size));
       order
   }
   ```

   关键是**稳定**：体积相等的兄弟节点保持发现顺序，否则每次流式重排都会让等大的行来回跳（jitter）。

2. **渲染时，把展示行号经置换映射到存储索引；`cursor` 是一个展示层坐标，不是存储索引。**

   ```rust
   // sorted=true 走体积降序；sorted=false 用恒等置换（静态/未排序视图共用同一函数）
   let order: Vec<usize> = if sorted {
       size_desc_order(&node.children)
   } else {
       (0..node.children.len()).collect()
   };
   // abs_idx 是展示行号，order[abs_idx] 才是存储位置
   let child = &node.children[order[abs_idx]];
   ```

3. **只有当动作要触碰真实树时，才用置换把展示坐标翻译回存储索引——而且一律走 `.get()`。**

   下钻（descend）：把当前 `cursor`（展示位）翻译成存储索引再 push 进 `nav_path`：

   ```rust
   let order = size_desc_order(&current_node.children);
   if let Some(&stored_idx) = order.get(*cursor) {
       if current_node.children.get(stored_idx).is_some_and(|c| !c.is_file) {
           nav_path.push(stored_idx); // nav_path 永远存"存储索引"
           *cursor = 0;               // 进入新层，光标回到最大项
       }
   }
   ```

   标记（mark）走完全相同的翻译链——`size_desc_order(...)` 后 `order.get(*cursor).and_then(|&i| children.get(i))` 拿到目标项，再取其路径；同样一律经 `.get()`，绝不用 `cursor` 直接索引 `children`。

**"跟住最大项"因此是免费的**：展示位 0 永远是最大的子项，所以每收到一条流式项，只要用户还没手动导航，就把光标钉在 0：

```rust
if !user_navigated {
    *cursor = 0; // 展示位 0 恒为最大项，自动跟随
}
```

**扫描结束的收尾（finalize）**：此时可以放心地**原地排一次序**（不再有增量索引在消费它）。之后用**同一个渲染函数**、传 `sorted = false`（恒等置换）来画静态视图，并把 `cursor` / `nav_path` / `cursor_stack` **全部重置到 root**——因为收尾的原地重排让所有 live 发现顺序索引都失效了，重置是最简单且正确的收敛方式。

## Why This Matters

- **单一数据源，零同步成本。** 树只存一种顺序（发现顺序），没有"排序后的副本"要和构建器保持一致，也就没有副本漂移的 bug。构建器、导航、渲染各自的不变量都不被破坏。
- **展示与结构解耦。** 想换展示规则（按名字、按修改时间、过滤隐藏项）只需换一个产出置换的纯函数，渲染和动作代码原封不动。这就是为什么这个模式能推广到本仓库之外的任何场景。
- **`.get()` 把 TOCTOU 竞态降级成 no-op。** 流式重排可能在"渲染算出 cursor"与"用户按键触发动作"之间悄悄改变顺序，让 cursor 指向一个已经变化的行。因为所有索引访问都走 `.get()`，最坏情况只是这次按键**什么都不做**，而非 panic 或误删。对 live 扫描而言，这种一瞬间的错位是可接受的，并且被约 200ms 的渲染节流天然限界。
- **纯函数 = 可测试。** `size_desc_order` 无副作用，可以直接对"降序""稳定""是 0..n 的合法排列""空输入"逐条单测（见 `analyzer.rs` 的测试），而不需要搭起整棵树或跑扫描。

## When to Apply

当**同一个按索引导航的、增量构建的结构**需要以一种不同于其存储顺序的方式展示时，就用这个模式。触发信号：

- 结构是 **append-only / 增量**填充的，且有**存量索引**引用着固定位置（游标、路径栈、构建器指针、撤销栈……）；
- 你想改变的只是**呈现顺序**（排序、过滤、分组），而非数据本身；
- 展示顺序可能**随时间变化**（流式、实时更新），原地重排会在"读顺序"和"用顺序"之间开一个竞态窗口。

反过来，**不适用/无需**此模式的情况：结构一旦构建就冻结、不再有增量索引消费它（比如 finalize 之后），那就直接原地排序更简单——本例的收尾正是这样做的，并把渲染切到恒等置换。

一句话判据：**只要"展示顺序"和"结构索引顺序"有一方会独立变化，就把展示顺序表达为一层可丢弃的置换，而不是去改结构。**

## Examples

**通用骨架**（把"结构索引"和"展示坐标"当两种坐标系，置换是它们之间的翻译）：

```rust
// 1) 纯函数：产出展示顺序置换，绝不 mutate
fn display_order<T, K: Ord>(items: &[T], key: impl Fn(&T) -> K) -> Vec<usize> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| key(&items[i])); // 稳定排序，等值保持原顺序
    order
}

// 2) 渲染：display_row -> storage_index
let order = display_order(&items, |x| std::cmp::Reverse(x.size));
for (display_row, &storage_idx) in order.iter().enumerate() {
    draw(display_row, &items[storage_idx]);
}

// 3) 动作：把 cursor（展示坐标）翻回 storage_index，一律 .get()
if let Some(&storage_idx) = order.get(cursor) {
    if let Some(item) = items.get(storage_idx) {
        act_on(item); // 只有到这里才触碰真实结构
    }
}
```

**边界要点**，直接决定这个模式好不好用：

- 用**稳定**排序（`sort_by_key` / `sort_by`，不是 `sort_unstable_*`），否则等值元素在实时重排下会抖。
- `cursor`、`display_row` 属于展示坐标系；`nav_path`、`depth_stack`、`storage_idx` 属于结构坐标系。**跨系必须经置换翻译**，永不混用。
- 所有跨系访问走 `.get()` 返回 `Option`，把"陈旧坐标"这种竞态收敛为安全的 no-op。
- 结构一旦冻结，就可以退化为原地排序 + 恒等置换，复用同一条渲染路径（`sorted: bool` 参数即可），不必为静态视图另写一套。

## Related

- **源码**：`crates/tui/src/ui/analyzer.rs`（`size_desc_order`、`render_children_list` 的置换逻辑及其单元测试）、`crates/tui/src/lib.rs`（`handle_analyze_entry` 的跟随最大项、live 下钻/标记的坐标翻译、`SortDone` 收尾重置）、`crates/tui/src/app.rs`（`AppState::AnalyzingLive` / `Analyzing`）。
- **GitHub Issue #3（已关闭）"perf: Analyzer 架构优化待办"** 中的子项 **#9「finalize() 递归排序异步化」**：该 todo 关注的是 finalize 阶段一次性原地递归排序在 100 万节点下阻塞主线程（200–500ms）。本模式的显示层置换是对同一问题的架构性回应——实时阶段完全不需要原地排序，只有收尾才排一次；若进一步把渲染改为视口内置换（Issue #4「render_children_list 视口优化」同处一个函数），连收尾的全量排序都可按需化。
- 相关工作项见 `docs/ideation/2026-07-04-tui-ux-maturity.md` 的 #4（扫描中实时排序 + 跟随最大项）。
