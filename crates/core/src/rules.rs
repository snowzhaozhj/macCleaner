use crate::models::SafetyLevel;
use std::path::PathBuf;

/// 路径匹配模式
#[derive(Debug, Clone)]
pub enum PathPattern {
    /// 匹配一个精确的绝对路径（如 ~/Library/Caches）
    Exact(PathBuf),
    /// 匹配目录树中任意位置具有此名称的目录（如 "node_modules"）
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

/// 获取 home 目录，如果获取失败则 panic
fn home() -> PathBuf {
    dirs::home_dir().expect("无法获取用户 home 目录")
}

/// 系统缓存清理规则（全部为 Safe）
pub fn clean_rules() -> Vec<CleanRule> {
    let home = home();
    vec![
        CleanRule {
            name: "System Caches".into(),
            description: "系统及应用缓存文件".into(),
            patterns: vec![PathPattern::Exact(home.join("Library/Caches"))],
            safety: SafetyLevel::Safe,
            category: "系统缓存".into(),
        },
        CleanRule {
            name: "Application Logs".into(),
            description: "应用日志文件".into(),
            patterns: vec![PathPattern::Exact(home.join("Library/Logs"))],
            safety: SafetyLevel::Safe,
            category: "系统缓存".into(),
        },
        CleanRule {
            name: "System Temp Files".into(),
            description: "系统临时文件".into(),
            patterns: vec![
                PathPattern::Exact(PathBuf::from("/tmp")),
                PathPattern::Exact(PathBuf::from("/private/var/folders")),
            ],
            safety: SafetyLevel::Safe,
            category: "系统缓存".into(),
        },
        CleanRule {
            name: "Chrome Cache".into(),
            description: "Google Chrome 浏览器缓存".into(),
            patterns: vec![PathPattern::Exact(
                home.join("Library/Caches/Google/Chrome"),
            )],
            safety: SafetyLevel::Safe,
            category: "浏览器缓存".into(),
        },
        CleanRule {
            name: "Safari Cache".into(),
            description: "Safari 浏览器缓存".into(),
            patterns: vec![PathPattern::Exact(
                home.join("Library/Caches/com.apple.Safari"),
            )],
            safety: SafetyLevel::Safe,
            category: "浏览器缓存".into(),
        },
        CleanRule {
            name: "Firefox Cache".into(),
            description: "Firefox 浏览器缓存".into(),
            patterns: vec![PathPattern::Exact(home.join("Library/Caches/Firefox"))],
            safety: SafetyLevel::Safe,
            category: "浏览器缓存".into(),
        },
    ]
}

/// 开发产物清理规则
pub fn purge_rules() -> Vec<CleanRule> {
    let home = home();
    vec![
        CleanRule {
            name: "node_modules".into(),
            description: "Node.js 依赖目录".into(),
            patterns: vec![PathPattern::DirName("node_modules".into())],
            safety: SafetyLevel::Safe,
            category: "Node.js".into(),
        },
        // 注意：Rust target 目录需要在扫描阶段额外验证父目录是否包含 Cargo.toml
        CleanRule {
            name: "Rust target".into(),
            description: "Rust 编译产物目录（仅匹配含 Cargo.toml 的项目）".into(),
            patterns: vec![PathPattern::DirName("target".into())],
            safety: SafetyLevel::Safe,
            category: "Rust".into(),
        },
        CleanRule {
            name: "Python venv".into(),
            description: "Python 虚拟环境目录".into(),
            patterns: vec![
                PathPattern::DirName(".venv".into()),
                PathPattern::DirName("venv".into()),
            ],
            safety: SafetyLevel::Safe,
            category: "Python".into(),
        },
        CleanRule {
            name: "__pycache__".into(),
            description: "Python 字节码缓存目录".into(),
            patterns: vec![PathPattern::DirName("__pycache__".into())],
            safety: SafetyLevel::Safe,
            category: "Python".into(),
        },
        CleanRule {
            name: "dist/build".into(),
            description: "前端构建产物目录".into(),
            patterns: vec![
                PathPattern::DirName("dist".into()),
                PathPattern::DirName("build".into()),
            ],
            safety: SafetyLevel::Moderate,
            category: "Build Output".into(),
        },
        CleanRule {
            name: ".gradle".into(),
            description: "Gradle 构建缓存目录".into(),
            patterns: vec![PathPattern::DirName(".gradle".into())],
            safety: SafetyLevel::Safe,
            category: "Gradle".into(),
        },
        CleanRule {
            name: "DerivedData".into(),
            description: "Xcode 构建缓存".into(),
            patterns: vec![PathPattern::Exact(
                home.join("Library/Developer/Xcode/DerivedData"),
            )],
            safety: SafetyLevel::Safe,
            category: "Xcode".into(),
        },
        CleanRule {
            name: "Pods".into(),
            description: "CocoaPods 依赖目录".into(),
            patterns: vec![PathPattern::DirName("Pods".into())],
            safety: SafetyLevel::Safe,
            category: "CocoaPods".into(),
        },
    ]
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
            .map(|n| n == name)
            .unwrap_or(false),
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
        for rule in &rules {
            match rule.name.as_str() {
                "dist/build" => assert_eq!(
                    rule.safety,
                    SafetyLevel::Moderate,
                    "dist/build 应为 Moderate"
                ),
                _ => assert_eq!(
                    rule.safety,
                    SafetyLevel::Safe,
                    "清理规则 '{}' 应为 Safe",
                    rule.name
                ),
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
}
