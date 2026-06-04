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

/// 清理规则
#[derive(Debug, Clone)]
pub struct CleanRule {
    pub name: String,
    pub description: String,
    pub patterns: Vec<PathPattern>,
    pub safety: SafetyLevel,
    pub category: String,
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
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PatternEntry {
    Absolute { absolute: String },
    Exact { exact: String },
    DirName { dir_name: String },
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
            CleanRule {
                name: entry.name,
                description: entry.description,
                patterns,
                safety: entry.safety,
                category: entry.category,
            }
        })
        .collect()
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
        let rules = purge_rules();
        let moderate_rules = [
            "dist/build",
            "Docker Desktop Data",
            "Maven Repository",
            "Xcode Archives",
            "Android AVD/SDK",
        ];
        for rule in &rules {
            if moderate_rules.contains(&rule.name.as_str()) {
                assert_eq!(
                    rule.safety,
                    SafetyLevel::Moderate,
                    "'{}' 应为 Moderate",
                    rule.name
                );
            } else {
                assert_eq!(
                    rule.safety,
                    SafetyLevel::Safe,
                    "'{}' 应为 Safe",
                    rule.name
                );
            }
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
            "Android AVD/SDK",
            "JetBrains Cache",
        ];
        for name in &expected {
            assert!(names.contains(name), "缺少规则: {name}");
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
                "Android AVD/SDK" => assert_eq!(rule.category, "Android"),
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
