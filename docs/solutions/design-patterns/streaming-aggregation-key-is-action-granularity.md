---
title: "流式聚合的键必须是动作粒度（路径），而非展示粒度（分类）"
date: 2026-07-05
category: design-patterns
module: mc-core
problem_type: design_pattern
component: service_object
severity: high
applies_when:
  - "边扫描/流式产出，边按某个键累加增量（delta）后上报给 UI"
  - "同一条底层实体（如一个 PathBuf）可能被多个规则/分类命中"
  - "下游用户动作（勾选、删除、计数）作用在实体本身，而非分类标签上"
  - "展示分组的键与执行动作的键不是同一维度"
symptoms:
  - "同一 PathBuf 以两个不同分类各出现一条可勾选条目（系统缓存 35.94GB + 浏览器缓存 1.63GB 都指向 ~/Library/Caches）"
  - "勾选耦合：勾其中一条，另一条跟着翻转"
  - "计数三方打架：header 已选 4 项 vs toast 已标记 3 项 vs 确认清单同路径两行"
  - "用户以为删 1.63GB 子项，marked 的却是整个根目录（危险的过度删除）"
root_cause: logic_error
resolution_type: code_fix
tags:
  - streaming
  - aggregation-key
  - identity-key
  - scanner
  - delta-emit
  - deletion-granularity
  - tui
---

# 流式聚合的键必须是动作粒度（路径），而非展示粒度（分类）

> 注：`component: service_object` 是 schema 枚举里对"无 UI 的后端引擎"最接近的取值；本模式的实际载体是 `mc-core` 的流式扫描引擎（`scanner.rs`），与 Rails 无关。姊妹文档 [[render-layer-sort-permutation-indices]] 讲的是同一枚硬币的另一面——**展示坐标 vs 存储坐标**；本文讲的是**聚合身份键 vs 展示分组键**。

## Context

macCleaner 的 Clean 扫描是**流式**的：`scan_with_rules` 边遍历文件边把 size 累加进一个 `HashMap`，每攒够一批就 `flush` 一次，向 UI 上报各聚合项相对上次的**增量（delta）**，让 TUI 列表边扫边填充。UI 侧对同一个 `(category, path)` 的重复 `Found` 事件做合并累加，最终各项的大小与"一次性上报"完全一致。

规则表里存在**同根下的子规则**：根规则 `~/Library/Caches`（分类"系统缓存"）之下，还有更具体的子规则命中 `~/Library/Caches/Google/Chrome`、`com.apple.Safari`、`Firefox`（分类"浏览器缓存"）。文件按**最长前缀**归入最具体的规则，这一步是对的。

出问题的是**聚合与上报用的键**。原实现把累加器和 flush 都按**分类名**索引：

```rust
let mut size_by_category: HashMap<String, u64> = ...;
// flush 时：所有分类都以 root_path 上报
reporter.on_event(meta.found(root_path.to_path_buf(), delta));
```

于是子规则"浏览器缓存"虽然 size 归类正确，`Found` 事件却**顶着根路径 `~/Library/Caches` emit**。TUI 按 `(category, path)` 合并后，得到两个 `path` 相同、`category` 不同的条目——同一个 `PathBuf` 出现两次。

这引发一连串信任崩塌：勾选耦合（marked 是 `HashSet<PathBuf>`，两条共享同一路径，勾一个另一个跟着变）、header/toast/确认清单三方计数打架、以及最危险的一条——**用户以为在删 1.63GB 的浏览器缓存，`marked` 里实际是整个 `~/Library/Caches`（35.94GB）**。展示上的"分类"骗过了用户，动作作用的却是路径。

## Guidance

**流式聚合的键，必须选下游动作真正作用的那个维度（这里是删除粒度 = `PathBuf`），而不是方便展示分组的那个维度（分类名）。** 展示分组可以从实体派生，但实体的身份不能反过来从展示标签重建。

判据一句话：**如果两条聚合项会被同一次用户动作（勾选/删除）区别对待，它们就必须有不同的聚合键。** 分类名做不到这点（同根下多个分类可以共享根路径），路径能。

落地就是把累加器和 meta 映射整体从"按分类名索引"换成"按匹配基路径索引"：

```rust
// 累加器的键是 PathBuf（匹配基路径），不是分类名
let mut size_by_base: HashMap<PathBuf, u64> = HashMap::new();

// 最长前缀匹配时，连同基路径一起取出
let (base, meta) = root.children.iter().rev()
    .find(|(child_path, _)| path.starts_with(child_path))
    .map_or_else(|| (&root.path, &root.meta), |(p, m)| (p, m));
*size_by_base.entry(base.clone()).or_insert(0) += size;

// flush 时 emit 到真实基路径，而非统一顶着 root_path
reporter.on_event(meta.found(base.clone(), delta));
```

换键之后，"浏览器缓存"条目获得真实子路径 `~/Library/Caches/Google/Chrome` → 独立可勾可删；勾选耦合、清单重复、计数失真**同时消失**。TUI 端 `(category, path)` 的合并逻辑一行没改——因为病根从来不在展示层。

**顺带删掉两处不再需要的东西**（换键让它们失去存在理由，别留着腐烂）：

- 原来靠"同名分类 safety 唯一"这条脆弱不变式撑着的 `debug_assert!`——按路径索引后，同名分类允许有不同 safety，断言过时。
- `Meta::fallback`——原来按分类名查 meta 可能查不到需要兜底；按路径索引时"`size_by_base` 的键必然来自 `base_meta`"是结构保证的强不变式，`base_meta[base]` 直接索引即可，兜底成了死代码。

## Why This Matters

- **身份键选错，是"数据安全"级的错，不是"显示瑕疵"级的错。** 表面症状是"列表里多一行"，实质是删除动作作用的对象与用户认知不符——在一个清理工具里，这直接通向"删了不该删的"。展示层怎么合并、怎么去重都救不回来，因为源头 emit 的 `path` 就是错的。
- **换键让整类 bug 一次性蒸发，而不是逐个打补丁。** 勾选耦合、三方计数不一致、确认清单重复——看起来是三个独立 bug，其实是同一个错误键的三种投影。修键，三者同时消失；反过来若在展示层逐个 hack（去重、对齐计数、防耦合），只会越修越复杂，且永远漏。
- **强不变式换掉脆弱不变式。** 旧代码靠 `debug_assert` 守着"同名分类 safety 唯一"——一个规则表当前恰好成立、将来加个规则就可能被打破的约定。新键让"聚合键来自 meta 映射"成为**结构上必然**的不变式，兜底和断言随之都成死代码，可以删。**换键顺带简化，是正确换键的标志**；如果换完还得加一堆防御，多半是键还没选对。
- **可回归。** 病根在"emit 的 path"，回归测试就得断言 path 本身，而非只断言 size 求和（旧测试只测了后者，恰好放过了这个 bug）。

## When to Apply

任何**流式/增量聚合再上报**的场景，聚合键的选择先问一句：**下游拿这个键做什么？**

- 如果下游要对聚合项做**区别性动作**（勾选、删除、跳转、单独操作），键必须是那个动作的作用对象（实体身份：id、路径、主键），哪怕展示时还想按别的维度分组。
- 展示分组的键是**派生**的、可从实体算出来的（category 从 path 的规则匹配得出）；反过来用展示键当聚合键，就等于把多个实体塞进一个桶，动作时无法再拆开。

**触发信号**：同一条底层实体可能被多个规则/标签命中；UI 上"一行"却对应后台"一个可独立操作的对象"；你发现要在展示层写"去重""防止两行联动"的逻辑——这通常是聚合键选在了展示粒度上的**下游症状**，回头改键比在展示层堵漏更省。

**不适用**：聚合纯粹是为了**只读汇总展示**（如"各分类总大小"统计面板），下游没有针对单项的区别性动作——那按展示维度聚合本就是对的，无需下沉到实体粒度。

## Examples

**Before（错误：展示粒度做聚合键）** — 两个分类共享根路径，emit 出两条同 `PathBuf` 条目：

```rust
let mut size_by_category: HashMap<String, u64> = HashMap::new();
*size_by_category.entry(meta.category.clone()).or_insert(0) += size;
// ...flush:
reporter.on_event(meta.found(root_path.to_path_buf(), delta)); // 所有分类都顶 root_path
```

结果（TUI 合并后）：

```
● 系统缓存   ~/Library/Caches   35.94 GB   [x]
● 浏览器缓存 ~/Library/Caches    1.63 GB   [x]   ← 同一 PathBuf，勾选耦合
```

**After（正确：动作粒度做聚合键）** — emit 到真实基路径：

```rust
let mut size_by_base: HashMap<PathBuf, u64> = HashMap::new();
*size_by_base.entry(base.clone()).or_insert(0) += size;
// ...flush:
reporter.on_event(meta.found(base.clone(), delta)); // emit 到匹配基路径
```

结果：

```
● 系统缓存   ~/Library/Caches                       34.31 GB   [x]
● 浏览器缓存 ~/Library/Caches/Google/Chrome          1.63 GB   [x]   ← 独立路径，独立可勾
```

**回归测试断言的是 path 而非 size**（这是关键——size 求和在两种实现下都对，只有 path 集合能抓住 bug）：

```rust
// SizeReporter 额外记录每个分类收到的 Found 路径集合
let paths = found_paths.lock().unwrap();
assert_eq!(paths.get("根缓存"), Some(&HashSet::from([root.clone()])),
    "根分类 Found 应只挂在 root 基路径上");
assert_eq!(paths.get("子缓存"), Some(&HashSet::from([sub.clone()])),
    "子分类 Found 应挂在其真实子路径 sub 上（而非顶着 root）");
```

## Related

- **源码**：`crates/core/src/scanner.rs`（`scan_with_rules` 的 `base_meta`/`size_by_base` 累加、`flush_base_deltas` 上报、`scan_clean_streams_multiple_categories_under_one_root` P0 回归测试）。
- **提交**：`4a7130b fix(core): 流式 Found 按匹配基路径而非分类名归属 (KTD1/P0)`。
- **计划**：`docs/plans/2026-07-05-008-fix-tui-ux-round3-trust-chain-plan.md`（KTD1 根因分析 + 全轮 TUI 信任链修复）；体检来源 `.impeccable/critique/2026-07-05T08-36-47Z__crates-tui.md`。
- **姊妹模式** [[render-layer-sort-permutation-indices]]：同样是"别把展示维度和底层身份/结构混为一谈"——那篇管**展示顺序 vs 存储索引**（渲染层置换），本篇管**展示分组键 vs 聚合身份键**（emit 的 path）。两者都指向一条更大的原则：**展示是底层实体的可派生视图，绝不能反过来定义实体的身份或位置。**
- **领域词汇**：CONCEPTS.md「匹配基路径（Base Path）」——本模式在本仓的稳定命名。
