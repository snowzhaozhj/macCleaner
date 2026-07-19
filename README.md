# macCleaner

快速、安全的 Mac 清理工具。

## 孤儿残留清理（反向卸载）

`mc orphans` 扫描 `~/Library` 里**父应用已不存在**的残留——你装过又删掉的应用，
往往在 Caches / Preferences / Application Support 等目录留下没有主人的文件。

这与 `mc uninstall` 互补：`uninstall` 是**正向**（选一个仍装着的应用，删它的残留）；
`orphans` 是**反向**（枚举残留，反查父应用是否还在）。

```bash
mc orphans              # 列出孤儿残留 + 证据，交互勾选后移废纸篓
mc orphans --dry-run    # 只预览，不删除
mc orphans --json       # 结构化输出（脚本友好）
mc orphans --permanent  # 永久删除而非移废纸篓（不可恢复）
```

### 安全约束

反向卸载天生比正向保守——你没有主动选择要删任何东西，是工具替你发现的：

- **一律不预选**：孤儿残留（即便安全档是 Safe）永远不默认勾选。应用虽已卸载，
  你可能有意保留其数据（例如日后重装恢复配置）。`mc orphans --yes` **不会**自动删除
  任何孤儿——它会提示你去掉 `--yes` 交互指定要删的编号。
- **三道误杀防线**：① 从残留条目名析不出 bundle-id（普通目录名如 `Google`）→ 跳过；
  ② 系统预留 / 共享前缀（`com.apple.*` 等）→ 绝不当孤儿；③ 残留目录距今不足 30 天
  → 给缓冲期跳过（可能是刚删、马上要重装）。宁可漏报，不可误杀。
- **默认移废纸篓**：可恢复；`--permanent` 例外（永久、不可恢复）。

## 用户叠加规则

macCleaner 的清理规则内置于二进制、随发布审计与测试。你可以在
`~/.config/mc/rules.toml` 追加**本地叠加规则**，让 `mc clean` / `mc purge`
扫描到内置规则未覆盖的目录——无需等待发版。

### 格式

```toml
# Exact 规则：精确路径（clean 扫描采用）。
# exact 相对 $HOME；也可用 absolute 给绝对路径。
# 注：safety 字段需填写以通过解析，但用户规则运行时一律被降为 Risky（见下方安全约束），
# 故此处写什么值都不影响实际安全档。
[[rules]]
name = "mytool-cache"
description = "MyTool 缓存目录"
category = "自定义缓存"
safety = "Safe"
impact = "缓存文件，工具下次运行会重建"
recovery = "重新运行 MyTool 即自动重建"
preselect = false
patterns = [{ exact = "Library/Caches/mytool" }]

# DirName 规则：按目录名匹配（purge 扫描采用），必须配 root_markers 守卫，
# 否则整棵树按目录名误报。sibling = 同级需存在该文件；inside = 目录内需存在该文件。
[[rules]]
name = "mytool-build"
description = "MyTool 构建产物"
category = "自定义开发产物"
safety = "Moderate"
impact = "构建输出，重新构建即可再生"
recovery = "重新运行构建命令"
preselect = false
root_markers = [{ sibling = "mytool.config" }]
patterns = [{ dir_name = ".mytool-build" }]
```

### 安全约束

用户规则只扩大**扫描发现范围**，不改变本产品的删除安全模型：

- **永不预选**：无论 TOML 里写 `preselect = true` 与否，加载层一律强制
  `preselect = false`。用户规则命中项永远不会被 `--yes` 或默认勾选自动删除，
  必须在交互界面手动逐项勾选。
- **自声明 safety 一律降为 Risky**：用户规则里写的 `safety` 是**未经审计**的，加载层
  一律强制为 `Risky`。这样未审计的用户项落入最保守档——不被"全选安全项"扫入、删除
  必经 type-to-confirm，不能靠自声明 `safety = "Safe"` 绕过安全确认。（TOML 里的
  `safety` 字段仍需填写以通过解析，但运行时不被信任。）
- **不能降级删除授权**：用户规则**不能**把任意路径的删除风险降为 Safe/Moderate。
  Analyze 等任意路径删除入口只信任内置规则；未匹配内置规则的路径按 Risky 处理。
- **fail-closed**：TOML 解析失败，或任一条 `DirName` 规则缺 `root_markers` 守卫，
  整个用户规则文件被跳过（扫描退化为纯内置规则），并在日志打出原因——默认安全
  优先于"尽量加载"。
- **DirName 必配守卫**：按目录名匹配的规则必须声明 `root_markers`（`sibling` 或
  `inside`），否则整棵树按目录名匹配、误报炸裂——这条由加载门禁强制拒绝。

成功加载 ≥1 条用户规则时，`mc clean` / `mc purge` 会在扫描前提示"已加载 N 条
用户叠加规则"。文件不存在则静默。TUI 同样会看到用户规则命中项（带永不预选语义）。
