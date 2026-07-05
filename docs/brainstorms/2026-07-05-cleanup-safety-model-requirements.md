---
id: brainstorm-007
title: 清理安全分级模型重构
date: 2026-07-05
status: accepted
---

# 清理安全分级模型重构

## 概述

重做清理功能的安全分级模型：把分级轴从模糊的"删了麻不麻烦"改为清晰的**"删了会不会丢不可再生数据"**，并配一份成文 rubric，让每条规则的评级有据可依、跨语言一致。每条规则新增"删了会怎样 + 如何恢复"的证据文案，界面逐项展示；同时给按目录名匹配的规则补项目根守卫，消除误报。目标是让用户**看得懂依据、信得过分级、敢按下确认**——而不牺牲一键清理的释放量。

## 问题背景

当前 `SafetyLevel`（`Safe`/`Moderate`/`Risky`）硬编码在两个 TOML 里，驱动配色、形状标记（●/▲/✕）和默认勾选（`models.rs:29`，`selected = safety == Safe`）。审查现状发现四个相互关联的问题：

1. **分级依据没成文 → 跨语言不一致。** 下载缓存里 Maven `.m2` 被标 `Moderate`，而同类的 Cargo/Go/npm/pip/Homebrew 全是 `Safe`；构建产物里 Rust `target` 是 `Safe`，`dist`/`build` 却是 `Moderate`。同类东西评级相反，纯拍脑袋。

2. **匹配不精确 → 扫出不合理的东西。** 只有 `target` 有项目根守卫（`scanner.rs:342`，检查同级 `Cargo.toml`），且是硬编码特例。`dist`/`build`/`Pods`/`venv`/`node_modules` 全无守卫，纯按目录名匹配——`dist`/`build` 又是常见英文词，用户任意目录下同名文件夹都会被扫进来。

3. **分级把两件事混成一轴 → 真正危险的被低估。** CONCEPTS 里 `Moderate`="删后需重新获取"（恢复代价轴），`Risky`="需谨慎"（数据丢失轴），两把尺子混用。结果 Docker `Data/vms`（删除抹掉所有镜像/容器/命名卷，卷内数据可能不可恢复）和 Xcode Archives（含已发布 App 的 dSYM，删了无法符号化线上崩溃）这些真有丢数据风险的项，都藏在 `Moderate`——当前没有一条规则是 `Risky`。

4. **看不到依据 → 不敢删几十 G。** 每条只有一行 `description`，界面只用颜色/形状表达等级，从不说明"为什么安全、删了会失去什么、怎么拿回来"。几十 G 的 Safe 项默认全勾上，用户自然发怵。

## 关键决策

- **分级轴 = 数据丢失风险，"重建代价"降为信息字段。** 分级只回答"删了会不会丢不可再生的东西"，不再混入"重建麻不麻烦"。重建代价改由独立的证据文案（如"需重新下载""需重新 build"）承载，供用户判断但不参与配色/默认勾选。这样 node_modules、target、下载缓存这些几十 G 的大件仍属零丢失、仍默认勾选，释放量不受影响。

- **保留三档、不上双轴矩阵。** TUI 保持轻量、认知负担低。代价是"恢复代价"不进配色，只在逐项文案里体现——评估认为对一个"想快速清理"的工具，这个取舍值得。

- **A+C 融合形态：单轴风险驱动配色/默认勾选 + 每条规则必填证据文案。** 分级提供可扫描的信号，证据文案提供可信的依据，二者互补。

- **默认勾选边界 = 零丢失即勾（含大件），有丢失风险不勾。** `Safe` 与 `Moderate` 均为零数据丢失，均默认勾选；`Risky` 默认不勾、删除时额外确认。因为上移到 `Risky` 的项（Docker 卷 / dSYM / AVD）本就没被默认勾过，故本次重构**默认释放量基本不变**——只修信任，不砍卖点。

## 成文分级 Rubric（R1）

**R1** — 三档定义成文，写入代码注释与规则文件头部，作为评级唯一判据：

- **Safe（●，绿，默认勾选）** — 删除零数据丢失，且下次需要时**自动、透明、按需**补回，用户无需任何显式动作、不留下被破坏的项目。典型：共享/下载缓存。
- **Moderate（▲，黄，默认勾选）** — 删除零数据丢失，但会清空某个项目的完整依赖/构建产物，下次构建/运行前需用户**显式跑一次重装或重建命令**（一次完整冷重建，非按需）。典型：项目本地依赖与构建目录。
- **Risky（✕，红，默认不勾，删除额外确认）** — 删除可能丢失**不可再生数据或有价值状态**。典型：虚拟机磁盘/命名卷、含 dSYM 的归档、装好环境的模拟器镜像。

判据口诀：**删了会不会丢不可再生的东西？** 会 → Risky。不会，但要用户手动重建一个项目 → Moderate。不会，且自动按需补回 → Safe。

## 按 Rubric 重评级（R2–R4）

**R2** — 系统缓存规则（`clean_rules.toml`）全部维持 `Safe`：`Library/Caches`、`Library/Logs`、`/tmp`、Chrome/Safari/Firefox 缓存——均为自动按需重建的缓存。

**R3** — 开发产物规则（`purge_rules.toml`）按下表重评级。**"变化"列标出与现状的差异，是本需求的核心交付**：

| 规则 | 现状 | 新级别 | 依据 | 变化 |
|---|---|---|---|---|
| 下载缓存：Homebrew、Maven `.m2`、Go mod、Cargo registry/git、npm/pnpm/yarn、pip、JetBrains、`.gradle` | 多为 Safe，Maven=Moderate | **Safe** | 共享缓存，按需透明重下，不破坏任何项目 | Maven 由 Moderate→Safe，其余不变 → **消除跨语言不一致** |
| `__pycache__` | Safe | **Safe** | 运行时自动重建，无感 | 不变 |
| `node_modules` | Safe | **Moderate** | 清空项目依赖，需 `npm/pnpm/yarn install` | Safe→Moderate |
| Rust `target` | Safe | **Moderate** | 清空项目构建产物，需 `cargo build` 冷重建 | Safe→Moderate |
| Python `venv`/`.venv` | Safe | **Moderate** | 清空虚拟环境，需重建 + `pip install` | Safe→Moderate |
| `Pods` | Safe | **Moderate** | 清空 CocoaPods 依赖，需 `pod install` | Safe→Moderate |
| `DerivedData` | Safe | **Moderate** | Xcode 项目构建缓存，删后首次构建为冷重建 | Safe→Moderate |
| `dist`/`build` | Moderate | **Moderate** | 项目构建输出，需重新 build | 不变 |
| Docker Desktop Data（`Data/vms`） | Moderate | **Risky** | 抹掉全部镜像/容器/命名卷，卷内数据（如数据库）不可恢复 | Moderate→Risky |
| Xcode Archives | Moderate | **Risky** | 含已发布 App 的 dSYM，删后无法符号化线上崩溃日志，不可再生 | Moderate→Risky |
| Android AVD（`.android/avd`） | Moderate | **Risky** | 模拟器镜像含装好的环境/应用状态，重建耗时且状态丢失 | Moderate→Risky |

**R4** — 拆分 "Android AVD/SDK" 规则。当前一条规则同时匹配 `.android/avd`（有价值状态，Risky）与 `Library/Android/sdk/.temp`（临时文件，Safe），级别混淆。拆为两条独立规则，各自评级。

## 匹配精度：项目根守卫（R5）

**R5** — 每条按目录名（`dir_name`）匹配的规则必须带**项目根标记守卫**，把 `target` 的 `Cargo.toml` 特例泛化为通用机制：命中目录只有在同级/父级存在对应项目标记时才计入结果。建议标记（最终以实现校准）：

| 规则 | 项目根标记 |
|---|---|
| `node_modules` | 同级 `package.json` |
| `target` | 同级 `Cargo.toml`（现有，保留） |
| `venv`/`.venv` | 目录内含 `pyvenv.cfg`（venv 规范标记） |
| `dist`/`build` | 同级项目标记（如 `package.json`）——误报风险最高，守卫最关键 |
| `Pods` | 同级 `Podfile` |

`__pycache__` 名称足够独特、误报风险低，可维持无守卫（如实现简单也可加"含 `.pyc`"校验）。

## 证据文案：让依据可见（R6–R7）

**R6** — 每条规则新增两个**必填非空**字段，逐项在界面展示：
- `impact`（删了会怎样）——一句话描述删除后果，最坏情况优先。
- `recovery`（如何恢复）——一句话描述恢复方式，不可恢复的明确写"不可恢复"。

示例：
- `node_modules` → impact:"该项目依赖被清空，下次构建/运行前需重装"；recovery:"项目目录运行 `npm/pnpm/yarn install`"
- Docker Data → impact:"所有本地镜像、容器、命名卷被删除，卷内数据（如数据库）不可恢复"；recovery:"镜像可重新 `pull`；命名卷内数据无法恢复"

**R7** — 结果列表逐项展示 `impact`/`recovery`（至少在选中/展开项上），删除确认框对 `Risky` 项醒目呈现其 `impact`。文案位置与展开形式的细节交由 TUI 层设计。

## 默认勾选与删除确认（R8–R9）

**R8** — 默认勾选 `Safe` + `Moderate`（均零丢失）；`Risky` 默认不勾。`Risky` 项**仍正常显示**在结果中（不隐藏），仅不预选，与"无静默删除、全透明"一致。

**R9** — 当待删集合中包含 `Risky` 项，删除确认环节需**额外强调**——在确认框中单独列出 Risky 项及其 `impact`，要求用户明确知情后再执行。具体交互（二次确认 / 高亮 / 独立分区）交由 TUI 层设计。

## 验收示例

- **零丢失大件仍默认勾选**：扫描出 40G `node_modules`（Moderate）+ 12G Cargo registry（Safe），打开结果时两者均已勾选，界面标注 node_modules 为"需重装"、Cargo 为纯缓存 → 一键释放量与重构前一致。
- **危险项默认不勾且被解释**：扫描出 30G Docker `Data/vms`，标为 Risky（红/✕），**默认未勾选**，逐项文案写明"卷内数据不可恢复"；用户手动勾选后进入确认，确认框单独强调该项后果。
- **跨语言一致**：Maven `.m2` 与 Cargo registry、Go mod 现在同为 Safe、同样默认勾选、同样标注"按需重下"——同类同级。
- **误报被守卫拦截**：`~/Documents/photobook/build`（同级无 `package.json`）在 Purge 扫描中**不出现**；某前端项目 `~/code/web/dist`（同级有 `package.json`）正常出现并标 Moderate。
- **Archives 风险显性**：Xcode Archives 标为 Risky，文案写明"删后无法符号化线上崩溃日志" → 用户看得到代价再决定。

## 成功标准

- 三档 rubric 成文并写入规则文件/代码注释，任何新增规则可据此自评。
- 单测断言：所有下载缓存类规则为 Safe；所有项目本地构建/依赖类规则为 Moderate；Docker vms、Xcode Archives、Android AVD 为 Risky。
- 单测断言：每条规则 `impact`/`recovery` 均非空；每条 `dir_name` 规则均配置了项目根守卫。
- 重构后默认勾选的总释放量 **不低于** 重构前（回归保护 STRATEGY 的"单次清理释放空间中位数"指标）。
- 用户能在结果界面直接读到每项的删除后果与恢复方式，无需查文档。

## 范围边界

**本次不做（留待后续）：**
- 用户自定义 / 可编辑规则——规则仍编译进二进制（`include_str!`）。让用户增删改规则或调整分级是自然延伸，但不在本次范围。
- 精确预估"重新下载多少 GB"——`recovery` 文案只定性描述（"需重新下载"），不含实际体积。
- 双轴矩阵分级模型——已评估否决，认知成本不值当。

## 假设

- **部分"是否丢数据"无法从路径静态判定**（如某 Docker 命名卷是否装了数据库、某 AVD 是否有重要状态）。这类一律按**最坏情况**标 `Risky` 并在文案中提示，宁可保守。
- 项目根标记（`package.json`/`Podfile`/`pyvenv.cfg` 等）足以区分真实项目产物与用户同名目录；极端情况下仍可能漏判/误判，可在实现/评审中按实际样本微调标记集。
- 现有 TUI 结果视图有足够空间逐项展示证据文案（或通过展开态）；若空间紧张，展示形式在 TUI 层降级处理，但 `impact`/`recovery` 字段本身必存。
