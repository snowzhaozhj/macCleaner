# Concepts

本项目的共享领域词汇——具有项目特定含义的实体、命名流程与状态概念，供 `docs/solutions/` 与 AGENTS.md 直接引用而无需重新定义。首次以核心领域词汇播种，之后随 ce-compound / ce-compound-refresh 处理学习而累积；也欢迎直接编辑。仅作术语表，不是规格说明或杂物间。

> 本次为播种范围：磁盘分析器与清理命令域（`mc-tui` 交互层）。其余领域词汇留待后续 learning 累积或 `ce-compound-refresh` 全量播种。

## 命令 (Commands)

macCleaner 对外暴露四个顶层命令，互为对照——前三个按规则匹配"可删项"，第四个只做浏览分析、不隐含删除规则。

### Analyze（磁盘分析）
交互式磁盘空间浏览器：把某个目录树按体积可视化、可逐层下钻，用来回答"谁在占空间"。区别于其余三个清理命令——它不套用可删规则，是一个纯探索/导航视图。扫描是流式的，边扫边可导航。

### Clean
按内置规则扫描并清理系统缓存、日志、临时文件的清理命令。

### Purge
扫描并清理开发产物（依赖目录、构建输出等）的清理命令。

### Uninstall
卸载应用并清理其残留文件的清理命令。

## 安全 (Safety)

### SafetyLevel（安全等级）
对每个可删项按**数据丢失风险**分级（判据口诀："删了会不会丢不可再生的东西"）——Safe（自动按需补回、零丢失，如共享/下载缓存）、Moderate（零丢失但需用户手动重建一个项目，如 node_modules、target、DerivedData）、Risky（可能丢失不可再生数据或有价值状态，如 Docker 命名卷、Xcode Archives 的 dSYM、装好环境的 AVD）。"重建代价"不进本轴，改由每项的证据文案（impact/recovery）承载。该等级驱动界面配色与形状标记（●/▲/✕）。**默认预选与等级解耦**：预选 = `safety != Risky && rule.preselect`——Safe/Moderate 默认勾选，Risky 默认不勾且删除时需 type-to-confirm；个别规则（如 `dist/build`）虽为 Moderate 但设 `preselect = false` 而不默认勾选。评级依据成文 rubric，见 `crates/core/src/models.rs` 的 `SafetyLevel` 文档注释与 `docs/brainstorms/2026-07-05-cleanup-safety-model-requirements.md`。

## 选择与删除 (Selection & Deletion)

### 统一标记集（Marked）
用户跨"结果列表"与"磁盘分析器"共享的同一份"待删除路径"集合。所有删除动作只作用于该集合，它是"选择"与"删除"之间的单一事实来源；标记是全局的（一处标记，各视图可见），与过滤视图相互独立。

### 移废纸篓（Trash 删除）
本产品的删除语义：命中项一律移入系统废纸篓（可恢复），不做不可逆的永久删除。删除确认框会先列出完整待删路径清单再执行。
