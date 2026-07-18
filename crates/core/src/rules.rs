use crate::models::SafetyLevel;
use serde::Deserialize;
use std::path::PathBuf;

/// 路径匹配模式
#[derive(Debug, Clone)]
pub enum PathPattern {
    /// 匹配一个精确的绝对路径（如 ~/Library/Caches）
    Exact(PathBuf),
    /// 匹配目录树中任意位置具有此名称的目录（如 "`node_modules`"）
    DirName(String),
}

/// 项目根标记：仅当匹配目录满足这些标记时才计入结果（消除按目录名匹配的误报）。
/// 组合子默认 AND（全部命中）——见 `matches_root_markers`。
#[derive(Debug, Clone)]
pub enum RootMarker {
    /// 标记存在于匹配目录的**父级**（如 `node_modules` 旁的 `package.json`）
    Sibling(String),
    /// 标记存在于匹配目录**内部**（如 `venv` 内的 `pyvenv.cfg`）
    Inside(String),
}

/// 清理规则
#[derive(Debug, Clone)]
pub struct CleanRule {
    pub name: String,
    pub description: String,
    pub patterns: Vec<PathPattern>,
    pub safety: SafetyLevel,
    pub category: String,
    /// 删除后果一句话（"删了会怎样"）。
    pub impact: String,
    /// 恢复方式一句话（"如何恢复"）。
    pub recovery: String,
    /// 仅对 `DirName` 模式生效的项目根守卫；空表示不设守卫（如 `__pycache__`）。
    pub root_markers: Vec<RootMarker>,
    /// 是否默认预选。默认 true；`dist/build` 等设 false（仍扫出、仍可手动勾）。
    pub preselect: bool,
}

// --- TOML 反序列化中间结构 ---

#[derive(Deserialize)]
struct RuleFile {
    rules: Vec<RuleEntry>,
}

#[derive(Deserialize)]
struct RuleEntry {
    name: String,
    description: String,
    category: String,
    safety: SafetyLevel,
    patterns: Vec<PatternEntry>,
    #[serde(default)]
    impact: String,
    #[serde(default)]
    recovery: String,
    #[serde(default)]
    root_markers: Vec<MarkerEntry>,
    #[serde(default = "default_true")]
    preselect: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PatternEntry {
    Absolute { absolute: String },
    Exact { exact: String },
    DirName { dir_name: String },
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MarkerEntry {
    Sibling { sibling: String },
    Inside { inside: String },
}

fn home() -> PathBuf {
    crate::platform::get_home_dir()
}

/// 解析规则 TOML。返回 `Err` 而非 panic：内置规则由调用方 `expect`（编译期烘进二进制的
/// 坏 TOML 属程序 bug），**用户规则**则据此 fail-closed 优雅跳过而非崩溃整个进程。
fn parse_rules_toml(toml_str: &str, source: &str) -> Result<Vec<CleanRule>, String> {
    let home = home();
    let file: RuleFile =
        toml::from_str(toml_str).map_err(|e| format!("{source} 解析失败: {e}"))?;
    let rules = file
        .rules
        .into_iter()
        .map(|entry| {
            let patterns = entry
                .patterns
                .into_iter()
                .map(|p| match p {
                    PatternEntry::Absolute { absolute } => {
                        PathPattern::Exact(PathBuf::from(absolute))
                    }
                    PatternEntry::Exact { exact } => PathPattern::Exact(home.join(exact)),
                    PatternEntry::DirName { dir_name } => PathPattern::DirName(dir_name),
                })
                .collect();
            let root_markers = entry
                .root_markers
                .into_iter()
                .map(|m| match m {
                    MarkerEntry::Sibling { sibling } => RootMarker::Sibling(sibling),
                    MarkerEntry::Inside { inside } => RootMarker::Inside(inside),
                })
                .collect();
            CleanRule {
                name: entry.name,
                description: entry.description,
                patterns,
                safety: entry.safety,
                category: entry.category,
                impact: entry.impact,
                recovery: entry.recovery,
                root_markers,
                preselect: entry.preselect,
            }
        })
        .collect();
    Ok(rules)
}

/// 判断按目录名命中的目录是否满足其规则的项目根守卫（默认 AND：全部命中）。
/// `matched_dir` 是被命中的目录本身（如 `.../node_modules`）。空守卫恒真。
pub fn matches_root_markers(markers: &[RootMarker], matched_dir: &std::path::Path) -> bool {
    markers.iter().all(|m| match m {
        RootMarker::Sibling(name) => matched_dir
            .parent()
            .is_some_and(|parent| parent.join(name).exists()),
        RootMarker::Inside(name) => matched_dir.join(name).exists(),
    })
}

/// 系统缓存清理规则（从 `clean_rules.toml` 加载）
pub fn clean_rules() -> Vec<CleanRule> {
    static TOML: &str = include_str!("clean_rules.toml");
    parse_rules_toml(TOML, "clean_rules.toml").expect("内置 clean_rules.toml 应始终有效")
}

/// 开发产物清理规则（从 `purge_rules.toml` 加载）
pub fn purge_rules() -> Vec<CleanRule> {
    static TOML: &str = include_str!("purge_rules.toml");
    parse_rules_toml(TOML, "purge_rules.toml").expect("内置 purge_rules.toml 应始终有效")
}

/// 用户规则安全 lint 门禁（纯函数，便于单测）。通过返回 `Ok`，违规返回 `Err(原因)`。
///
/// 只做一项**正确性**校验：含 `DirName` 模式却无 `root_markers` → 拒绝（否则整树按目录名匹配、
/// 误报炸裂）。**不做数据目录黑名单**——防「误删用户数据」的职责由加载层无条件的
/// `preselect = false` 强制承担（见 `user_rules_from_str`）：用户规则永不自动预选、永不被
/// `--yes`/默认勾选自动删除，需用户在 TUI 里手动逐项勾选。故硬拒绝路径既非必要也无谓限制了
/// 用户对自己数据的清理自由。同理**不检查 preselect**——它由加载层强制，不是拒绝判据。
pub fn validate_user_rule(rule: &CleanRule) -> Result<(), String> {
    let has_dirname = rule
        .patterns
        .iter()
        .any(|p| matches!(p, PathPattern::DirName(_)));
    if has_dirname && rule.root_markers.is_empty() {
        return Err(format!(
            "DirName 规则 '{}' 必须配置非空 root_markers（否则整树按目录名匹配、误报）",
            rule.name
        ));
    }
    Ok(())
}

/// 从 TOML 文本加载用户规则并跑安全门禁。**fail-closed**：TOML 解析失败或**任一条**规则违反
/// `DirName` 守卫，都跳过整个文件（返回空），并 `log::error!` 打出具体原因——默认安全优先于「尽量加载」。
///
/// 通过的规则被无条件强制两件事，共同封住「用户自声明安全等级」这个信任缺口：
/// 1. `preselect = false`——永不自动预选，即便声明 preselect=true；`--yes`/默认勾选不删。
/// 2. `safety = Risky`——用户规则的 safety 是**自声明、未经审计**的，不能作为任何删除界面的
///    信任输入。若沿用声明值，一条 `safety="Safe"` 的规则会被 TUI 的 `select_all_safe`（按
///    `safety != Risky` 全选）扫入待删集、且 `confirm_has_risky` 放行为普通确认，绕过 Risky 的
///    type-to-confirm（安全审查发现）。强制 Risky 让未审计用户项落入最保守档：永不预选、全选安全
///    项时被排除、删除必经 type-to-confirm——与 `analyze-unknown-path-deletion-fail-closed` 学习
///    对「未知路径」的处理一致。用户仍可在 TUI 逐项确认删除，只是不再享有「安全」快捷路径。
fn user_rules_from_str(toml_str: &str, source: &str) -> Vec<CleanRule> {
    let rules = match parse_rules_toml(toml_str, source) {
        Ok(r) => r,
        Err(e) => {
            log::error!("用户规则 {source} 解析失败，已跳过全部用户规则: {e}");
            return Vec::new();
        }
    };
    for rule in &rules {
        if let Err(reason) = validate_user_rule(rule) {
            log::error!("用户规则未通过安全门禁，已跳过全部用户规则（fail-closed）: {reason}");
            return Vec::new();
        }
    }
    rules
        .into_iter()
        .map(|mut r| {
            r.preselect = false;
            r.safety = SafetyLevel::Risky;
            r
        })
        .collect()
}

/// 加载用户本地叠加规则 `~/.config/mc/rules.toml`（不存在则返回空）。读文件失败/门禁不过均
/// 优雅降级为空。真正的解析+门禁逻辑在 `user_rules_from_str`（可脱离真实 home 单测）。
pub fn user_rules() -> Vec<CleanRule> {
    let path = home().join(".config/mc/rules.toml");
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(&path) {
        Ok(s) => user_rules_from_str(&s, "~/.config/mc/rules.toml"),
        Err(e) => {
            log::error!("读取用户规则 {} 失败，已跳过: {e}", path.display());
            Vec::new()
        }
    }
}

/// 返回所有规则（系统缓存 + 开发产物 + 通过门禁的用户叠加规则）。
/// 用户规则 append 在末尾：扫描保持内置规则的既有顺序；证据反查另按风险与具体度选取。
pub fn all_rules() -> Vec<CleanRule> {
    let mut rules = builtin_rules();
    rules.extend(user_rules());
    rules
}

/// 内置规则全集（clean + purge），不含用户本地叠加规则。
fn builtin_rules() -> Vec<CleanRule> {
    let mut rules = clean_rules();
    rules.extend(purge_rules());
    rules
}

/// 为任意路径（如磁盘分析器中用户手动选中的路径）查找规则证据：命中某条规则的模式时
/// 返回其 `(safety, impact, recovery)`，否则 `None`。`None` 只表示「无规则证据」，调用方
/// 不应据此推断路径是 Safe；删除场景应使用 [`deletion_evidence_for_path`]。
///
/// 用途：供只读诊断解释某条路径命中了哪条规则；它包含用户叠加规则，不能据此授权删除。
/// Analyze 等删除入口必须改用 [`deletion_evidence_for_path`]。
pub fn evidence_for_path(path: &std::path::Path) -> Option<(SafetyLevel, String, String)> {
    let rules = all_rules();
    evidence_for_path_in_rules(path, &rules)
}

/// 为删除场景返回 fail-closed 的安全证据。已知路径原样沿用规则证据；未知路径无法证明可安全
/// 重建，因此按 Risky 处理，并明确提示潜在的数据损失与恢复边界。Analyze 只信任随二进制
/// 审计、测试过的内置规则：用户规则用于扩展扫描范围，不能作为任意路径降级为 Safe/Moderate
/// 的依据。
pub fn deletion_evidence_for_path(path: &std::path::Path) -> (SafetyLevel, String, String) {
    let rules = builtin_rules();
    deletion_evidence_for_path_in_rules(path, &rules)
}

/// 批量返回 Analyze 删除证据，保持输入顺序；内置规则只加载一次，避免每个标记路径重复解析。
pub fn deletion_evidence_for_paths(paths: &[PathBuf]) -> Vec<(SafetyLevel, String, String)> {
    let rules = builtin_rules();
    paths
        .iter()
        .map(|path| deletion_evidence_for_path_in_rules(path, &rules))
        .collect()
}

fn deletion_evidence_for_path_in_rules(
    path: &std::path::Path,
    rules: &[CleanRule],
) -> (SafetyLevel, String, String) {
    evidence_for_path_in_rules(path, rules).unwrap_or_else(|| {
        (
            SafetyLevel::Risky,
            "此路径未匹配任何已知清理规则，删除可能造成不可再生的用户数据或应用状态丢失".into(),
            "若仍在废纸篓可移回原处；清空废纸篓后，数据可能无法恢复".into(),
        )
    })
}

fn evidence_for_path_in_rules(
    path: &std::path::Path,
    rules: &[CleanRule],
) -> Option<(SafetyLevel, String, String)> {
    rules
        .iter()
        .filter_map(|rule| rule_match_specificity(rule, path).map(|specificity| (rule, specificity)))
        .reduce(|best, candidate| {
            let best_priority = (safety_rank(best.0.safety), best.1);
            let candidate_priority = (safety_rank(candidate.0.safety), candidate.1);
            if candidate_priority > best_priority {
                candidate
            } else {
                best
            }
        })
        .map(|(rule, _)| {
            (
                rule.safety,
                rule.impact.clone(),
                rule.recovery.clone(),
            )
        })
}

const fn safety_rank(safety: SafetyLevel) -> u8 {
    match safety {
        SafetyLevel::Safe => 0,
        SafetyLevel::Moderate => 1,
        SafetyLevel::Risky => 2,
    }
}

/// 返回规则对该路径最具体的命中程度。Exact 以基路径长度衡量；DirName 命中路径本身，
/// 因而具体度等于目标路径长度。DirName 还必须是未跟随符号链接看到的真实目录，并满足守卫。
fn rule_match_specificity(rule: &CleanRule, path: &std::path::Path) -> Option<usize> {
    rule.patterns
        .iter()
        .filter_map(|pattern| {
            if !matches_pattern(pattern, path) {
                return None;
            }
            match pattern {
                // Exact 保持「命中基路径或其后代」的原语义。
                PathPattern::Exact(base) => Some(base.components().count()),
                PathPattern::DirName(_) => std::fs::symlink_metadata(path)
                    .ok()
                    .filter(|metadata| metadata.file_type().is_dir())
                    .filter(|_| matches_root_markers(&rule.root_markers, path))
                    .map(|_| path.components().count()),
            }
        })
        .max()
}

/// 判断给定路径是否匹配某个模式
pub fn matches_pattern(pattern: &PathPattern, path: &std::path::Path) -> bool {
    match pattern {
        PathPattern::Exact(base) => path.starts_with(base),
        PathPattern::DirName(name) => path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SafetyLevel;

    #[test]
    fn clean_rules_all_safe() {
        for rule in clean_rules() {
            assert_eq!(
                rule.safety,
                SafetyLevel::Safe,
                "清理规则 '{}' 应为 Safe",
                rule.name
            );
        }
    }

    #[test]
    fn purge_rules_safety_levels() {
        // 按 rubric 重评级：项目本地产物为 Moderate，可能丢数据/状态的为 Risky，其余（含所有下载缓存）为 Safe。
        let moderate = [
            "node_modules",
            "Rust target",
            "Python venv",
            "dist/build",
            "DerivedData",
            "Pods",
        ];
        let risky = ["Docker Desktop Data", "Xcode Archives", "Android AVD"];
        for rule in &purge_rules() {
            let name = rule.name.as_str();
            let expected = if moderate.contains(&name) {
                SafetyLevel::Moderate
            } else if risky.contains(&name) {
                SafetyLevel::Risky
            } else {
                SafetyLevel::Safe
            };
            assert_eq!(rule.safety, expected, "'{name}' 分级不符 rubric");
        }
    }

    #[test]
    fn all_download_caches_are_safe() {
        // 跨语言一致性：所有下载/共享缓存必须同为 Safe（含 Maven，历史上曾被误标 Moderate）。
        let caches = [
            "Maven Repository",
            "Homebrew Cache",
            "Go Module Cache",
            "Cargo Cache",
            "npm/pnpm/yarn Cache",
            "pip Cache",
            "JetBrains Cache",
            "Gradle Cache",
        ];
        let rules = purge_rules();
        for name in &caches {
            let rule = rules
                .iter()
                .find(|r| r.name == *name)
                .unwrap_or_else(|| panic!("缺少缓存规则: {name}"));
            assert_eq!(rule.safety, SafetyLevel::Safe, "下载缓存 '{name}' 应为 Safe");
        }
    }

    #[test]
    fn purge_rules_new_dev_rules_exist() {
        let rules = purge_rules();
        let names: Vec<&str> = rules.iter().map(|r| r.name.as_str()).collect();
        let expected = [
            "Docker Desktop Data",
            "Maven Repository",
            "Homebrew Cache",
            "Go Module Cache",
            "Cargo Cache",
            "npm/pnpm/yarn Cache",
            "pip Cache",
            "Xcode Archives",
            "Android AVD",
            "Android SDK Temp",
            "Gradle Cache",
            "JetBrains Cache",
        ];
        for name in &expected {
            assert!(names.contains(name), "缺少规则: {name}");
        }
    }

    // R4: 拆分 Android AVD / SDK Temp
    #[test]
    fn android_avd_sdk_split() {
        let rules = purge_rules();
        let avd = rules.iter().find(|r| r.name == "Android AVD").expect("缺少 Android AVD");
        let sdk = rules.iter().find(|r| r.name == "Android SDK Temp").expect("缺少 Android SDK Temp");
        assert_eq!(avd.safety, SafetyLevel::Risky, "AVD 应为 Risky");
        assert_eq!(sdk.safety, SafetyLevel::Safe, "SDK 临时文件应为 Safe");
    }

    // R6: 每条规则 impact/recovery 非空
    #[test]
    fn all_rules_evidence_non_empty() {
        for rule in builtin_rules() {
            assert!(!rule.impact.trim().is_empty(), "规则 '{}' impact 不能为空", rule.name);
            assert!(!rule.recovery.trim().is_empty(), "规则 '{}' recovery 不能为空", rule.name);
        }
    }

    // D1: .gradle 窄化为 .gradle/caches
    #[test]
    fn gradle_narrowed_to_caches() {
        // .gradle 必须窄化为 exact ~/.gradle/caches，不能整树 dir_name 匹配（否则删签名密钥/配置）。
        for rule in builtin_rules() {
            for p in &rule.patterns {
                if let PathPattern::DirName(name) = p {
                    assert_ne!(name, ".gradle", "不应存在 dir_name '.gradle' 整树规则");
                }
            }
        }
        let gradle = purge_rules()
            .into_iter()
            .find(|r| r.name == "Gradle Cache")
            .expect("缺少 Gradle Cache 规则");
        let ok = gradle.patterns.iter().any(|p| {
            matches!(p, PathPattern::Exact(path) if path.ends_with("caches") && path.to_string_lossy().contains(".gradle"))
        });
        assert!(ok, "Gradle Cache 应精确匹配 ~/.gradle/caches");
    }

    #[test]
    fn docker_buildx_split_to_safe_cache() {
        // Docker Data/vms 为 Risky（含卷）；buildx 缓存单列为 Safe，避免误标不可逆。
        let rules = purge_rules();
        let vms = rules.iter().find(|r| r.name == "Docker Desktop Data").expect("缺少 Docker Desktop Data");
        assert_eq!(vms.safety, SafetyLevel::Risky);
        assert!(
            !vms.patterns.iter().any(|p| matches!(p, PathPattern::Exact(x) if x.to_string_lossy().contains("buildx"))),
            "vms 规则不应再含 buildx"
        );
        let buildx = rules.iter().find(|r| r.name == "Docker buildx Cache").expect("缺少 Docker buildx Cache");
        assert_eq!(buildx.safety, SafetyLevel::Safe, "buildx 应为 Safe 缓存");
    }

    fn evidence_rule(
        name: &str,
        patterns: Vec<PathPattern>,
        safety: SafetyLevel,
        impact: &str,
        root_markers: Vec<RootMarker>,
    ) -> CleanRule {
        CleanRule {
            name: name.into(),
            description: "测试规则".into(),
            patterns,
            safety,
            category: "测试".into(),
            impact: impact.into(),
            recovery: format!("恢复 {name}"),
            root_markers,
            preselect: true,
        }
    }

    #[test]
    fn evidence_for_path_preserves_known_rule_evidence() {
        // 分析器用它把 Risky 路径（如 Xcode Archives）识别出来以触发 type-to-confirm。
        let archives = home().join("Library/Developer/Xcode/Archives/old.xcarchive");
        let rules = builtin_rules();
        let evidence =
            evidence_for_path_in_rules(&archives, &rules).expect("Archives 路径应命中规则");
        let (safety, impact, recovery) = &evidence;
        assert_eq!(*safety, SafetyLevel::Risky);
        assert!(!impact.is_empty() && !recovery.is_empty());
        assert_eq!(deletion_evidence_for_path(&archives), evidence);
    }

    #[test]
    fn deletion_evidence_for_unknown_path_is_risky_and_non_empty() {
        let unknown = home().join("Documents/notes.txt");
        let rules = Vec::new();
        assert!(
            evidence_for_path_in_rules(&unknown, &rules).is_none(),
            "未知路径不应伪造规则证据"
        );
        // 公开删除入口只加载内置规则；即使测试机存在用户叠加规则，Documents 仍不得被降级。
        let (safety, impact, recovery) = deletion_evidence_for_path(&unknown);
        assert_eq!(safety, SafetyLevel::Risky);
        assert!(!impact.trim().is_empty(), "未知路径必须解释删除影响");
        assert!(!recovery.trim().is_empty(), "未知路径必须解释恢复边界");
    }

    #[test]
    fn dirname_evidence_requires_root_marker() {
        let temp = tempfile::tempdir().expect("应创建临时目录");
        let matched_dir = temp.path().join("node_modules");
        std::fs::create_dir(&matched_dir).expect("应创建待匹配目录");
        let rule = CleanRule {
            name: "node_modules".into(),
            description: "d".into(),
            patterns: vec![PathPattern::DirName("node_modules".into())],
            safety: SafetyLevel::Moderate,
            category: "Node.js".into(),
            impact: "依赖被删除".into(),
            recovery: "重新安装依赖".into(),
            root_markers: vec![RootMarker::Sibling("package.json".into())],
            preselect: true,
        };

        assert!(
            evidence_for_path_in_rules(&matched_dir, std::slice::from_ref(&rule)).is_none(),
            "仅目录名相同但缺少项目标记时不应返回规则证据"
        );

        std::fs::write(temp.path().join("package.json"), "{}").expect("应写入项目标记");
        let (safety, impact, recovery) = evidence_for_path_in_rules(&matched_dir, &[rule])
            .expect("目录名和项目标记同时命中时应返回规则证据");
        assert_eq!(safety, SafetyLevel::Moderate);
        assert_eq!(impact, "依赖被删除");
        assert_eq!(recovery, "重新安装依赖");
    }

    #[test]
    fn dirname_regular_files_are_not_rule_evidence() {
        let temp = tempfile::tempdir().expect("应创建临时目录");
        let pycache = temp.path().join("__pycache__");
        let node_modules = temp.path().join("node_modules");
        std::fs::write(&pycache, "不是目录").expect("应创建同名普通文件");
        std::fs::write(temp.path().join("package.json"), "{}").expect("应写入项目标记");
        std::fs::write(&node_modules, "也不是目录").expect("应创建同名普通文件");
        let rules = purge_rules();

        assert!(
            evidence_for_path_in_rules(&pycache, &rules).is_none(),
            "普通文件 __pycache__ 不得被降级为 Safe"
        );
        assert!(
            evidence_for_path_in_rules(&node_modules, &rules).is_none(),
            "即使 marker 存在，普通文件 node_modules 也不得被降级为 Moderate"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dirname_symlinks_are_not_rule_evidence_even_with_marker() {
        let temp = tempfile::tempdir().expect("应创建临时目录");
        let real_modules = temp.path().join("real-modules");
        let node_modules = temp.path().join("node_modules");
        std::fs::create_dir(&real_modules).expect("应创建符号链接目标目录");
        std::os::unix::fs::symlink(&real_modules, &node_modules)
            .expect("应创建 node_modules 符号链接");
        std::fs::write(temp.path().join("package.json"), "{}")
            .expect("应写入有效项目标记");
        let rules = purge_rules();

        assert!(
            evidence_for_path_in_rules(&node_modules, &rules).is_none(),
            "即使 marker 有效，DirName 符号链接也不能被降级为已知清理目录"
        );
        assert_eq!(
            deletion_evidence_for_path_in_rules(&node_modules, &rules).0,
            SafetyLevel::Risky,
            "符号链接必须走未知路径的 fail-closed 分级"
        );
    }

    #[test]
    fn higher_safety_wins_over_more_specific_rule_regardless_of_order() {
        let temp = tempfile::tempdir().expect("应创建临时目录");
        let project = temp.path().join("project");
        let node_modules = project.join("node_modules");
        std::fs::create_dir_all(&node_modules).expect("应创建依赖目录");
        std::fs::write(project.join("package.json"), "{}").expect("应写入项目标记");
        let mut rules = vec![
            evidence_rule(
                "宽泛 Risky",
                vec![PathPattern::Exact(project)],
                SafetyLevel::Risky,
                "宽泛 Risky 证据",
                vec![],
            ),
            evidence_rule(
                "具体 Safe",
                vec![PathPattern::DirName("node_modules".into())],
                SafetyLevel::Safe,
                "具体 Safe 证据",
                vec![RootMarker::Sibling("package.json".into())],
            ),
        ];

        for _ in 0..2 {
            let (safety, impact, _) = evidence_for_path_in_rules(&node_modules, &rules)
                .expect("宽泛 Exact 与具体 DirName 均应命中");
            assert_eq!(
                safety,
                SafetyLevel::Risky,
                "更高风险必须压过更具体的 Safe 规则"
            );
            assert_eq!(impact, "宽泛 Risky 证据");
            rules.reverse();
        }
    }

    #[test]
    fn same_safety_prefers_longest_exact_prefix() {
        let temp = tempfile::tempdir().expect("应创建临时目录");
        let broad = temp.path().join("cache");
        let specific = broad.join("app");
        let target = specific.join("data.bin");
        let mut rules = vec![
            evidence_rule(
                "宽泛 Exact",
                vec![PathPattern::Exact(broad)],
                SafetyLevel::Safe,
                "宽泛证据",
                vec![],
            ),
            evidence_rule(
                "具体 Exact",
                vec![PathPattern::Exact(specific)],
                SafetyLevel::Safe,
                "具体证据",
                vec![],
            ),
        ];

        for _ in 0..2 {
            let (safety, impact, _) =
                evidence_for_path_in_rules(&target, &rules).expect("两条 Exact 规则均应命中");
            assert_eq!(safety, SafetyLevel::Safe);
            assert_eq!(impact, "具体证据", "同风险时必须选最长前缀，不受规则顺序影响");
            rules.reverse();
        }
    }

    #[test]
    fn batch_deletion_evidence_matches_single_path_helper() {
        let paths = vec![
            home().join("Library/Developer/Xcode/Archives/old.xcarchive"),
            home().join("Documents/mc-batch-unknown.txt"),
        ];
        let batch = deletion_evidence_for_paths(&paths);
        let singles: Vec<_> = paths
            .iter()
            .map(|path| deletion_evidence_for_path(path))
            .collect();

        assert_eq!(batch, singles, "批量分类应与逐路径分类一致且保持顺序");
    }

    // D2: dist/build 默认不勾选
    #[test]
    fn dist_build_not_preselected() {
        for rule in purge_rules() {
            if rule.name == "dist/build" {
                assert!(!rule.preselect, "dist/build 应默认不勾选");
            } else {
                assert!(rule.preselect, "'{}' 应默认勾选", rule.name);
            }
        }
    }

    // R5: 每条 dir_name 规则（除 __pycache__）都配置了项目根守卫
    #[test]
    fn dirname_rules_have_guards() {
        // 除 __pycache__ 外，每条 dir_name 规则都必须配置项目根守卫。
        for rule in builtin_rules() {
            let has_dirname = rule
                .patterns
                .iter()
                .any(|p| matches!(p, PathPattern::DirName(_)));
            if has_dirname && rule.name != "__pycache__" {
                assert!(
                    !rule.root_markers.is_empty(),
                    "dir_name 规则 '{}' 必须配置 root_markers",
                    rule.name
                );
            }
        }
    }

    #[test]
    fn purge_rules_categories_correct() {
        let rules = purge_rules();
        for rule in &rules {
            match rule.name.as_str() {
                "Docker Desktop Data" => assert_eq!(rule.category, "Docker"),
                "Maven Repository" => assert_eq!(rule.category, "Java"),
                "Homebrew Cache" => assert_eq!(rule.category, "Homebrew"),
                "Go Module Cache" => assert_eq!(rule.category, "Go"),
                "Cargo Cache" => assert_eq!(rule.category, "Rust"),
                "npm/pnpm/yarn Cache" => assert_eq!(rule.category, "Node.js"),
                "pip Cache" => assert_eq!(rule.category, "Python"),
                "Xcode Archives" => assert_eq!(rule.category, "Xcode"),
                "Android AVD" | "Android SDK Temp" => assert_eq!(rule.category, "Android"),
                "JetBrains Cache" => assert_eq!(rule.category, "JetBrains"),
                _ => {}
            }
        }
    }

    #[test]
    fn exact_pattern_matches_subdirectory() {
        let pattern = PathPattern::Exact(PathBuf::from("/tmp"));
        assert!(matches_pattern(&pattern, &PathBuf::from("/tmp/foo/bar")));
        assert!(matches_pattern(&pattern, &PathBuf::from("/tmp")));
        assert!(!matches_pattern(&pattern, &PathBuf::from("/var/tmp")));
    }

    #[test]
    fn dirname_pattern_matches_by_filename() {
        let pattern = PathPattern::DirName("node_modules".into());
        assert!(matches_pattern(
            &pattern,
            &PathBuf::from("/project/node_modules")
        ));
        assert!(matches_pattern(
            &pattern,
            &PathBuf::from("/a/b/c/node_modules")
        ));
    }

    #[test]
    fn dirname_does_not_match_hidden_variant() {
        let pattern = PathPattern::DirName("node_modules".into());
        assert!(!matches_pattern(
            &pattern,
            &PathBuf::from("/project/.node_modules")
        ));
    }

    #[test]
    fn no_rules_reference_user_data_paths() {
        let forbidden = ["Documents", "Desktop", "Downloads"];
        for rule in builtin_rules() {
            for pattern in &rule.patterns {
                if let PathPattern::Exact(p) = pattern {
                    let path_str = p.to_string_lossy();
                    for f in &forbidden {
                        assert!(
                            !path_str.contains(f),
                            "规则 '{}' 不应引用用户数据路径 {}",
                            rule.name,
                            f
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn all_rules_prefixes_builtin() {
        // `all_rules()` = clean + purge + user（用户规则 append 在末尾）。为环境非依赖，
        // 只验证「内置全集是 all_rules() 的前缀」：前 N 条与内置全集逐条同名。测试机若真有
        // `~/.config/mc/rules.toml`，也只会在末尾多出用户规则，本断言仍成立。
        let clean = clean_rules();
        let purge = purge_rules();
        let builtin_len = clean.len() + purge.len();
        let all = all_rules();
        assert!(all.len() >= builtin_len, "all_rules 至少含全部内置规则");
        let builtin_names: Vec<&str> = clean
            .iter()
            .chain(purge.iter())
            .map(|r| r.name.as_str())
            .collect();
        let all_prefix_names: Vec<&str> =
            all.iter().take(builtin_len).map(|r| r.name.as_str()).collect();
        assert_eq!(
            all_prefix_names, builtin_names,
            "all_rules() 前缀应逐条等于 clean+purge 内置全集"
        );
    }

    #[test]
    fn no_duplicate_rule_names() {
        let rules = builtin_rules();
        let mut seen = std::collections::HashSet::new();
        for rule in &rules {
            assert!(
                seen.insert(&rule.name),
                "重复的规则名: '{}'",
                rule.name
            );
        }
    }

    #[test]
    fn no_empty_patterns() {
        for rule in builtin_rules() {
            assert!(
                !rule.patterns.is_empty(),
                "规则 '{}' 的 patterns 不能为空",
                rule.name
            );
        }
    }

    // --- 用户规则安全门禁（issue #22）---

    /// 构造用户规则，避免各测试重复 boilerplate。
    fn user_rule(
        patterns: Vec<PathPattern>,
        safety: SafetyLevel,
        preselect: bool,
        markers: Vec<RootMarker>,
    ) -> CleanRule {
        CleanRule {
            name: "用户自定义".into(),
            description: "d".into(),
            patterns,
            safety,
            category: "c".into(),
            impact: "i".into(),
            recovery: "r".into(),
            root_markers: markers,
            preselect,
        }
    }

    #[test]
    fn user_rule_targeting_user_data_now_accepted() {
        // 黑名单已撤去：指向 ~/Documents 等用户数据的规则不再硬拒——防误删的职责由加载层
        // preselect=false 强制承担（见下 forces_preselect_false 测试），门禁只保证正确性。
        let rule = user_rule(
            vec![PathPattern::Exact(home().join("Documents/secret"))],
            SafetyLevel::Moderate,
            false,
            vec![],
        );
        assert!(
            validate_user_rule(&rule).is_ok(),
            "指向用户数据的规则应被受理（防误删靠 preselect=false，非硬拒）"
        );
    }

    #[test]
    fn user_data_rule_forced_preselect_false() {
        // 核心信任闸：即便用户规则指向 ~/Documents 且声明 preselect=true，加载后仍被受理，
        // 但 preselect 被强制为 false——永不自动预选/自动删除，只能在 TUI 手动勾选。
        let toml = r#"
[[rules]]
name = "My Docs Overlay"
description = "d"
category = "c"
safety = "Safe"
preselect = true
patterns = [{ exact = "Documents/old-exports" }]
"#;
        let rules = user_rules_from_str(toml, "test");
        assert_eq!(rules.len(), 1, "指向用户数据的规则应被加载（不再硬拒）");
        assert!(
            !rules[0].preselect,
            "指向用户数据的规则 preselect 必须被强制 false"
        );
    }

    #[test]
    fn user_rule_dirname_without_markers_rejected() {
        let rule = user_rule(
            vec![PathPattern::DirName("build".into())],
            SafetyLevel::Moderate,
            false,
            vec![],
        );
        assert!(
            validate_user_rule(&rule).is_err(),
            "DirName 无 root_markers 应被拒"
        );
    }

    #[test]
    fn valid_user_rule_passes() {
        // 合法：Exact 任意路径；DirName 配了 root_markers。
        let exact = user_rule(
            vec![PathPattern::Exact(home().join(".cache/myapp"))],
            SafetyLevel::Safe,
            false,
            vec![],
        );
        assert!(validate_user_rule(&exact).is_ok(), "合法 Exact 规则应通过");
        let dirname = user_rule(
            vec![PathPattern::DirName(".mybuild".into())],
            SafetyLevel::Moderate,
            false,
            vec![RootMarker::Sibling("myproject.toml".into())],
        );
        assert!(
            validate_user_rule(&dirname).is_ok(),
            "配了 root_markers 的合法 DirName 规则应通过"
        );
    }

    #[test]
    fn user_rules_from_str_forces_preselect_false() {
        // 即便声明 preselect=true，加载后也必须强制为 false；且自声明 safety 被强制为 Risky
        // （未审计的用户 safety 不能作为任何删除界面的信任输入——见 user_rules_from_str 文档）。
        let toml = r#"
[[rules]]
name = "My Cache"
description = "d"
category = "c"
safety = "Safe"
preselect = true
patterns = [{ exact = ".cache/myapp" }]
"#;
        let rules = user_rules_from_str(toml, "test");
        assert_eq!(rules.len(), 1, "合法规则应被加载");
        assert!(!rules[0].preselect, "用户规则 preselect 必须被强制为 false");
        assert_eq!(
            rules[0].safety,
            SafetyLevel::Risky,
            "用户自声明 safety 必须被强制为 Risky（封住 select_all_safe 的信任缺口）"
        );
    }

    #[test]
    fn user_rules_from_str_fail_closed_on_one_bad_rule() {
        // 一好一坏（坏 = DirName 无 root_markers）→ fail-closed，整个文件不加载（返回空）。
        let toml = r#"
[[rules]]
name = "Good"
description = "d"
category = "c"
safety = "Safe"
patterns = [{ exact = ".cache/good" }]

[[rules]]
name = "Bad DirName"
description = "d"
category = "c"
safety = "Safe"
patterns = [{ dir_name = "build" }]
"#;
        let rules = user_rules_from_str(toml, "test");
        assert!(rules.is_empty(), "任一条违规应导致整个文件被拒（fail-closed）");
    }

    #[test]
    fn user_rules_from_str_fail_closed_on_parse_error() {
        // 坏 TOML 不 panic，优雅返回空。
        let rules = user_rules_from_str("this is not = valid toml [[[", "test");
        assert!(rules.is_empty(), "解析失败应优雅降级为空而非崩溃");
    }

    #[test]
    fn readme_example_rules_pass_gate() {
        // 契约测试：README「用户叠加规则」小节的示例 TOML 必须能通过门禁并加载。
        // 防文档漂移——若示例失效（字段改名、守卫要求变化），此测试红。
        // 与 README.md 的示例保持逐字一致。
        let toml = r#"
[[rules]]
name = "mytool-cache"
description = "MyTool 缓存目录"
category = "自定义缓存"
safety = "Safe"
impact = "缓存文件，工具下次运行会重建"
recovery = "重新运行 MyTool 即自动重建"
preselect = false
patterns = [{ exact = "Library/Caches/mytool" }]

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
"#;
        let rules = user_rules_from_str(toml, "README 示例");
        assert_eq!(rules.len(), 2, "README 两条示例规则都应通过门禁并加载");
        assert!(
            rules.iter().all(|r| !r.preselect),
            "用户规则一律强制 preselect=false"
        );
        assert!(
            rules.iter().all(|r| r.safety == SafetyLevel::Risky),
            "用户规则自声明 safety（示例里的 Safe/Moderate）一律被强制为 Risky"
        );
    }
}
