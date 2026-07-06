//! 只读权限诊断（issue #23 的 `mc doctor` 内核）。
//!
//! 纯只读：对若干需要 Full Disk Access 的标准路径做 `read_dir` 探测，分类出
//! 可读 / 缺授权 / 不存在 / 其它错误。**绝不修改任何系统状态**，只回报结论供
//! CLI 渲染授权引导。区分"权限"与"其它 IO"是核心——只有权限类才提示去授权。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 单个路径的可达性诊断结论。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "detail")]
pub enum PathStatus {
    /// 可正常 `read_dir`。
    Readable,
    /// 因权限被拒（`PermissionDenied`）——这才是"需要授权"的信号。
    NoPermission,
    /// 路径不存在（`NotFound`）：并非缺授权，可能该机器就没这个目录。
    Missing,
    /// 其它 IO 错误（非权限、非不存在），保留原始描述便于排查。
    Error(String),
}

/// 一条路径探测结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub path: PathBuf,
    pub status: PathStatus,
}

/// 把 `io::ErrorKind` 归类为诊断状态。抽出成纯函数便于单测"权限 vs 其它 IO"的分流。
#[must_use]
pub fn classify(kind: std::io::ErrorKind) -> PathStatus {
    match kind {
        std::io::ErrorKind::PermissionDenied => PathStatus::NoPermission,
        std::io::ErrorKind::NotFound => PathStatus::Missing,
        other => PathStatus::Error(format!("{other:?}")),
    }
}

/// 只读探测单个路径：尝试 `read_dir`，成功即 `Readable`，失败按 kind 分类。
#[must_use]
pub fn probe(path: &Path) -> ProbeResult {
    let status = match std::fs::read_dir(path) {
        Ok(_) => PathStatus::Readable,
        Err(e) => classify(e.kind()),
    };
    ProbeResult {
        path: path.to_path_buf(),
        status,
    }
}

/// 批量探测（保持输入顺序）。
#[must_use]
pub fn probe_all(paths: &[PathBuf]) -> Vec<ProbeResult> {
    paths.iter().map(|p| probe(p)).collect()
}

/// 需要 Full Disk Access 才能读取的标准路径清单（相对当前用户 home）。
///
/// 取一组广为人知、稳定的 FDA 门控目录做探测样本；不追求穷尽（macOS 版本间边界有差异），
/// 够判断"是否已授权"即可。
#[must_use]
pub fn standard_fda_paths() -> Vec<PathBuf> {
    let home = crate::platform::get_home_dir();
    vec![
        home.join("Library/Mail"),
        home.join("Library/Safari"),
        home.join("Library/Messages"),
        home.join("Library/Application Support/MobileSync"),
        home.join("Library/Application Support/com.apple.TCC"),
        home.join("Library/Cookies"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use tempfile::tempdir;

    #[test]
    fn classify_splits_permission_from_other_io() {
        // 分流核心：只有 PermissionDenied → NoPermission；NotFound → Missing；其余 → Error。
        assert_eq!(classify(ErrorKind::PermissionDenied), PathStatus::NoPermission);
        assert_eq!(classify(ErrorKind::NotFound), PathStatus::Missing);
        assert!(matches!(classify(ErrorKind::Other), PathStatus::Error(_)));
    }

    #[test]
    fn probe_readable_and_missing() {
        let dir = tempdir().unwrap();
        // 真实存在的临时目录 → Readable
        assert_eq!(probe(dir.path()).status, PathStatus::Readable);
        // 不存在的路径 → Missing（而非误判成缺授权）
        let missing = dir.path().join("definitely_absent_xyz");
        assert_eq!(probe(&missing).status, PathStatus::Missing);
    }

    #[test]
    fn probe_all_preserves_order_and_mix() {
        let dir = tempdir().unwrap();
        let readable = dir.path().to_path_buf();
        let missing = dir.path().join("nope");
        let results = probe_all(&[readable.clone(), missing.clone()]);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, readable);
        assert_eq!(results[0].status, PathStatus::Readable);
        assert_eq!(results[1].status, PathStatus::Missing);
    }

    #[cfg(unix)]
    #[test]
    fn probe_no_permission_on_locked_dir() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

        // root 可穿透 0o000（CI 常以 root 跑）：若当前进程仍能读，说明环境不适合断言，跳过。
        if std::fs::read_dir(&locked).is_ok() {
            let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
            return;
        }
        assert_eq!(
            probe(&locked).status,
            PathStatus::NoPermission,
            "0o000 目录应诊断为缺授权"
        );
        // 复原权限，确保 tempdir 能被清理。
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
    }

    #[test]
    fn standard_paths_are_under_home_library() {
        let paths = standard_fda_paths();
        assert!(!paths.is_empty());
        assert!(
            paths.iter().all(|p| p.to_string_lossy().contains("Library")),
            "标准探测路径应都在 ~/Library 下"
        );
    }
}
