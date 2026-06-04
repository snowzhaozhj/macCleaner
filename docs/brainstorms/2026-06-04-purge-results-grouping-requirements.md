---
id: brainstorm-004
title: Purge 结果页按风险分区 + 大小排序
date: 2026-06-04
status: accepted
---

# Purge 结果页分组展示优化

## 概述

purge_rules 扩展到 18 条规则（10+ 个 category）后，结果页平铺列表变得冗长。通过按 SafetyLevel 分区 + 组内按大小降序排序 + 风险标题行分隔，同时解决定位难、优先级不明、视觉杂乱三个问题。

## 核心需求

### 1. 按 SafetyLevel 分区展示

- 第一区：Safe 级别的 category（标题行："安全 (可放心删除)"）
- 第二区：Moderate 级别的 category（标题行："中等风险 (删除后需重新下载)"）
- 如果未来有 Risky 级别，作为第三区展示
- 分区标题行不可选中，光标自动跳过

### 2. 组内按 total_size 降序排序

- 每个风险分区内，category 按该分区内的 total_size 从大到小排列
- 用户一眼看到最大的空间占用在最前面

### 3. 分区标题行视觉设计

- 格式：`────── {标签} ──────`，居中显示
- 颜色跟随风险等级：Safe=Green, Moderate=Yellow, Risky=Red
- 标题行占一行，不参与选择/展开交互

### 4. 空分区隐藏

- 如果某个 SafetyLevel 下没有扫描到任何结果，不显示该分区标题行

## 非需求（不做）

- 不引入"超级分组"（按语言/生态聚合）——10 个 category 通过排序+分区已经够用
- 不加搜索/过滤功能——规模未到需要的程度
- 不改数据模型 `CategoryGroup` / `ScanResult`——只改 TUI 展示层

## 受影响文件

- `crates/tui/src/app.rs` — `FlatRow` 枚举增加 `RiskHeader` 变体，`build_flat_rows()` 实现排序和分区逻辑
- `crates/tui/src/ui/results.rs` — 渲染 `RiskHeader` 行，光标跳过逻辑

## 验收标准

- [ ] Safe 和 Moderate category 分区显示，各区内按大小降序
- [ ] 分区标题行有对应颜色，光标不能停在标题行上
- [ ] 无结果的分区不显示标题行
- [ ] 现有交互（Space 选择、Tab 展开、a 全选安全项）行为不变
