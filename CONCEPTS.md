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

### Orphans（反向卸载）
扫描 `~/Library` 标准子目录，找出**父应用已不存在**的 bundle-id 残留（用户装了又删的应用留下的无主残留）的清理命令。与 [Uninstall] 语义互补：Uninstall 是**正向**（给定仍安装的应用 → 找它的残留），Orphans 是**反向**（枚举残留 → 反查父应用是否还在，即正向匹配规则的补集）。三道误杀防线：① fail-closed 析取——从条目名析不出 bundle-id（不含足够 `.` 的普通目录名如 `Google`）即跳过，宁漏报不误杀；② 系统预留黑名单——`com.apple.*` 等系统/共享前缀绝不当孤儿（`RESERVED_BUNDLE_PREFIXES`，首版只硬保 Apple，其余按真机误报迭代）；③ 龄阈值——残留目录 mtime 距今不足默认 30 天则跳过（刚删可能马上重装，给缓冲期）。分级沿用应用残留 rubric（USER_DATA 子目录 → Moderate + 证据文案，其余 → Safe），但**孤儿一律 `preselect = false`（含 Safe 项）**：用户没主动选择要删、且应用已卸载但数据可能是有意保留的，故永不默认删、`--yes` 也不自动删。实现见 `crates/core/src/app_resolver.rs` 的 `scan_orphans` 与 `docs/solutions/security-issues/orphan-leftover-scan-false-positive-defenses.md`。

## GUI 信息层级 (GUI Information Layers)

### 决策面孔（Decision Face）
渐进披露的第一层：只呈现足以回答“值不值得做”的类别、规模、数量等决策信号，让用户无需阅读实现细节即可完成去留判断；会改变安全判断的风险信息仍必须在行动前显式出现。

### 审查面孔（Audit Face）
渐进披露的第二层：围绕“对象到底是什么”呈现完整身份、证据、影响、恢复方式与权威系统核对入口；它增强核对能力，但不改变风险分级、删除授权或最终动作语义。

## 安全 (Safety)

### SafetyLevel（安全等级）
对每个可删项按**两条判据串联**分级（非单轴）：先问"会不会丢不可再生数据/有价值状态"（会 → Risky，如 Docker 命名卷、Xcode Archives 的 dSYM、装好环境的 AVD）；不丢的再问"重建是否需用户主动发起且有明显耗时/打断"——需要 → Moderate（如 node_modules、target、DerivedData 冷编译），自动透明补回 → Safe（如共享/下载缓存、IDE 索引）。Safe 与 Moderate 都是零数据丢失，把它俩分开的正是这条**重建摩擦**轴；每项的具体代价再由证据文案（impact/recovery）细化。该等级驱动界面配色与形状标记（●/▲/✕）。**默认预选与等级解耦**：预选 = `safety != Risky && rule.preselect`——Safe/Moderate 默认勾选，Risky 默认不勾且删除时需 type-to-confirm；个别规则（如 `dist/build`）虽为 Moderate 但设 `preselect = false` 而不默认勾选。评级依据成文 rubric，见 `crates/core/src/models.rs` 的 `SafetyLevel` 文档注释与 `docs/brainstorms/2026-07-05-cleanup-safety-model-requirements.md`。

### Analyze 未知路径（fail-closed）
Analyze 不按清理规则筛选，用户可标记任意文件或目录；因此 `evidence_for_path(path) == None` 只表示“没有规则证据”，**不表示 Safe**。Analyze 发起删除时统一走 `deletion_evidence_for_path(s)`：仅信任随二进制审计、测试过的内置规则，用户叠加规则不能把任意路径降为 Safe/Moderate；未知路径保守归为 Risky、展示通用数据丢失与废纸篓恢复边界，并强制 type-to-confirm。多条规则同时命中时以最高风险优先、同级取最具体规则；`DirName` 还必须命中真实目录并满足 `root_markers`，不能仅凭文件名降低风险。GUI 与 TUI 必须消费这同一个核心分类入口，禁止各自设置本地 Safe fallback；真正启动删除前还要再次分类，且口令只授权确认框当时已展示为 Risky 的具体路径——任何后来升级的路径都必须先展示新证据并重新确认。

### 用户叠加规则（User Overlay Rules）
用户在 `~/.config/mc/rules.toml` 追加的本地清理规则，扩展 `mc clean` / `mc purge` 的**扫描发现范围**，无需等待发版补规则。与内置规则的关键区别在于**信任边界**：用户规则只能让扫描"看见"更多目录，其自声明的属性（safety/preselect）**不被信任**。`scan_clean`/`scan_purge` 采用它（`clean_rules()`/`purge_rules()` + `user_rules()`），但 `deletion_evidence_for_path` 仍只信 `builtin_rules()`（见 [Analyze 未知路径]）。加载层四重护栏：① 无条件强制 `preselect = false`（永不被 `--yes`/默认勾选自动删除）；② 无条件强制 `safety = Risky`——自声明 safety 未经审计，若沿用会被 TUI `select_all_safe`（按 `safety != Risky` 全选）扫入待删集并以普通确认放行，绕过 type-to-confirm，故一律降为最保守档；③ **fail-closed**——坏 TOML 或任一条 `DirName` 规则缺 `root_markers` 守卫即整文件跳过（退化纯内置）；④ `DirName` 必配守卫（`sibling`/`inside`）。按 pattern 类型天然分流：`Exact` 归 clean 策略、`DirName` 归 purge 策略（purge 亦处理 base 目录下的 `Exact`）。成功加载时 CLI 在扫描前提示条数。

## 选择与删除 (Selection & Deletion)

### 统一标记集（Marked）
用户跨"结果列表"与"磁盘分析器"共享的同一份"待删除路径"集合。所有删除动作只作用于该集合，它是"选择"与"删除"之间的单一事实来源；标记是全局的（一处标记，各视图可见），与过滤视图相互独立。

### 移废纸篓（Trash 删除）
本产品的删除语义：命中项一律移入系统废纸篓（可恢复），不做不可逆的永久删除。删除确认框会先列出完整待删路径清单再执行。**Done 屏复述**"已移入废纸篓可恢复 / 清空废纸篓才真正释放磁盘空间"，避免用户发现磁盘空间未变而误以为工具失效。

### 匹配基路径（Base Path）
Clean 扫描流式上报 `Found` 时的**归属键**：每个清理项挂在它匹配到的规则的基路径上（根规则 = 其 `Exact` 路径，最长前缀子规则 = 子路径），而非分类名。这是删除粒度的键——若错用展示粒度的分类名做键，同根下的子规则（如"浏览器缓存" `~/Library/Caches/Google/Chrome`）会顶着父路径 `~/Library/Caches` 上报，导致同一 `PathBuf` 跨分类重复聚合、勾选耦合、计数失真。TUI 侧按 `(category, base_path)` 合并累加流式增量。

## 扫描与取消 (Scanning & Cancellation)

### 协作式取消（Cooperative Cancellation）
扫描/清理不被强制中断：每次操作安装一个全新的取消标志，核心在遍历与测量循环中周期性查询它，发现置位即尽快收尾退出。取消**不改变返回类型**——被取消的扫描仍以「部分结果」正常返回（未测完的目录可能以零体积占位）；事件流一侧则在取消后丢弃残余事件。

因此取消语义对消费方是**双通道不对称**的：只信事件流的消费方天然看不到被污染的部分结果；直接采信返回值的消费方必须先检查取消标志——被取消的返回值不得写入权威状态槽或授权后续删除，这是给引擎新增消费方时的固定审视点。旧操作持有自己那份标志，后续操作安装新标志不会「反取消」仍在收尾的旧操作。
