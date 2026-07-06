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
/// 所有通过的规则都被无条件强制 `preselect = false`——这是防自动删除的**唯一且充分**的主闸：
/// 用户规则永不自动预选，即便声明 preselect=true 也改回 false，仍可在 TUI 手动勾选。
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
/// 用户规则 append 在末尾：Clean 最长前缀归类、evidence 匹配均保持内置优先。
pub fn all_rules() -> Vec<CleanRule> {
    let mut rules = clean_rules();
    rules.extend(purge_rules());
    rules.extend(user_rules());
    rules
}

/// 为任意路径（如磁盘分析器中用户手动选中的路径）推断安全信息：命中某条规则的模式时
/// 返回其 `(safety, impact, recovery)`，否则 `None`（视为 Safe、无证据）。
///
/// 用途：让分析器发起的删除也能对 Risky 路径（Docker 卷、Xcode Archives、AVD 等）触发
/// type-to-confirm，而不是一律按 Safe 单键删除。
pub fn evidence_for_path(path: &std::path::Path) -> Option<(SafetyLevel, String, String)> {
    all_rules().into_iter().find_map(|rule| {
        rule.patterns
            .iter()
            .any(|p| matches_pattern(p, path))
            .then(|| (rule.safety, rule.impact.clone(), rule.recovery.clone()))
    })
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

    /// 内置规则全集（clean + purge），**不含**用户叠加规则。内置契约测试验证的是
    /// 「内置规则的不变量」，不能用 `all_rules()`——那会在测试机存在
    /// `~/.config/mc/rules.toml` 时被用户规则污染（环境依赖、脆弱）。这不是放宽契约
    /// （rubric 断言本身不变），只是把被遍历的集合收窄回内置。
    fn builtin_rules() -> Vec<CleanRule> {
        let mut r = clean_rules();
        r.extend(purge_rules());
        r
    }

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

    #[test]
    fn evidence_for_path_flags_risky_paths() {
        // 分析器用它把 Risky 路径（如 Xcode Archives）识别出来以触发 type-to-confirm。
        let archives = home().join("Library/Developer/Xcode/Archives/old.xcarchive");
        let (safety, impact, recovery) =
            evidence_for_path(&archives).expect("Archives 路径应命中规则");
        assert_eq!(safety, SafetyLevel::Risky);
        assert!(!impact.is_empty() && !recovery.is_empty());
        // 未命中任何规则的普通路径返回 None（分析器据此按 Safe/空证据处理）。
        assert!(evidence_for_path(&home().join("Documents/notes.txt")).is_none());
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
        // 即便声明 preselect=true，加载后也必须强制为 false。
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
}
