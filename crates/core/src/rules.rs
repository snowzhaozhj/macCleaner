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

fn parse_rules_toml(toml_str: &str, source: &str) -> Vec<CleanRule> {
    let home = home();
    let file: RuleFile =
        toml::from_str(toml_str).unwrap_or_else(|e| panic!("{source} 解析失败: {e}"));
    file.rules
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
        .collect()
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
    parse_rules_toml(TOML, "clean_rules.toml")
}

/// 开发产物清理规则（从 `purge_rules.toml` 加载）
pub fn purge_rules() -> Vec<CleanRule> {
    static TOML: &str = include_str!("purge_rules.toml");
    parse_rules_toml(TOML, "purge_rules.toml")
}

/// 返回所有规则（系统缓存 + 开发产物）
pub fn all_rules() -> Vec<CleanRule> {
    let mut rules = clean_rules();
    rules.extend(purge_rules());
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
        for rule in all_rules() {
            assert!(!rule.impact.trim().is_empty(), "规则 '{}' impact 不能为空", rule.name);
            assert!(!rule.recovery.trim().is_empty(), "规则 '{}' recovery 不能为空", rule.name);
        }
    }

    // D1: .gradle 窄化为 .gradle/caches
    #[test]
    fn gradle_narrowed_to_caches() {
        // .gradle 必须窄化为 exact ~/.gradle/caches，不能整树 dir_name 匹配（否则删签名密钥/配置）。
        for rule in all_rules() {
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
        for rule in all_rules() {
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
        for rule in all_rules() {
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
    fn all_rules_combines_clean_and_purge() {
        let clean_count = clean_rules().len();
        let purge_count = purge_rules().len();
        let all_count = all_rules().len();
        assert_eq!(all_count, clean_count + purge_count);
    }

    #[test]
    fn no_duplicate_rule_names() {
        let rules = all_rules();
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
        for rule in all_rules() {
            assert!(
                !rule.patterns.is_empty(),
                "规则 '{}' 的 patterns 不能为空",
                rule.name
            );
        }
    }
}
