use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use humansize::{format_size, DECIMAL};
use log::{debug, warn};

/// 获取用户主目录
pub fn get_home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/Users/unknown"))
}

/// 返回系统缓存/日志/临时文件相关路径
pub fn get_cache_paths() -> Vec<PathBuf> {
    let home = get_home_dir();
    vec![
        home.join("Library/Caches"),
        home.join("Library/Logs"),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/folders"),
    ]
}

/// 返回应用残留可能存在的 ~/Library 子目录
pub fn get_app_support_paths() -> Vec<PathBuf> {
    let home = get_home_dir();
    vec![
        home.join("Library/Application Support"),
        home.join("Library/Caches"),
        home.join("Library/Preferences"),
        home.join("Library/LaunchAgents"),
        home.join("Library/Saved Application State"),
        home.join("Library/Logs"),
        home.join("Library/WebKit"),
        home.join("Library/HTTPStorages"),
    ]
}

/// 检测是否拥有 Full Disk Access 权限
///
/// 尝试读取 ~/Library/Mail 目录，若返回 `PermissionDenied` 则无权限。
pub fn check_full_disk_access() -> bool {
    let mail_dir = get_home_dir().join("Library/Mail");
    match fs::read_dir(&mail_dir) {
        Ok(_) => {
            debug!("Full Disk Access 检测通过: 可读取 {mail_dir:?}");
            true
        }
        Err(e) => {
            if e.kind() == std::io::ErrorKind::PermissionDenied {
                warn!("无 Full Disk Access 权限: {e:?}");
                false
            } else {
                // 目录不存在等其他错误，不代表缺少权限
                debug!("Full Disk Access 检测: 目录可能不存在 ({e:?})");
                true
            }
        }
    }
}

/// 计算 ~/.Trash 目录的总大小（字节）
///
/// 权限不足时返回 Ok(0) 并记录警告，不视为错误。
pub fn get_trash_size() -> Result<u64> {
    let trash_dir = get_home_dir().join(".Trash");
    if !trash_dir.exists() {
        return Ok(0);
    }
    match calc_dir_size(&trash_dir) {
        Ok(size) => Ok(size),
        Err(e) => {
            warn!("无法计算废纸篓大小（可能缺少权限）: {e:?}");
            Ok(0)
        }
    }
}

/// 递归计算目录大小，使用 `symlink_metadata` 避免跟随符号链接
fn calc_dir_size(path: &PathBuf) -> Result<u64> {
    let mut total: u64 = 0;
    let entries = fs::read_dir(path).with_context(|| format!("无法读取目录: {path:?}"))?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("读取目录条目失败: {e:?}");
                continue;
            }
        };
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(m) => m,
            Err(e) => {
                warn!("获取元数据失败 {:?}: {:?}", entry.path(), e);
                continue;
            }
        };
        if meta.is_dir() {
            total += calc_dir_size(&entry.path()).unwrap_or(0);
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}

/// 将路径列表移到废纸篓（使用 trash crate）
pub fn move_to_trash(paths: &[PathBuf]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    trash::delete_all(paths).context("移动文件到废纸篓失败")?;
    debug!("已将 {} 个项目移到废纸篓", paths.len());
    Ok(())
}

/// 清空废纸篓：直接删除 ~/.Trash 下的所有内容
pub fn empty_trash() -> Result<()> {
    let trash_dir = get_home_dir().join(".Trash");
    if !trash_dir.exists() {
        debug!("废纸篓目录不存在，无需清空");
        return Ok(());
    }
    let entries = fs::read_dir(&trash_dir).context("无法读取废纸篓目录")?;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!("读取废纸篓条目失败: {e:?}");
                continue;
            }
        };
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                warn!("获取废纸篓条目元数据失败 {path:?}: {e:?}");
                continue;
            }
        };
        let result = if meta.is_dir() {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(e) = result {
            warn!("删除废纸篓条目失败 {path:?}: {e:?}");
        }
    }
    debug!("废纸篓已清空");
    Ok(())
}

/// 格式化文件大小为人类可读字符串（DECIMAL 格式，与 macOS Finder 一致）
pub fn fmt_size(bytes: u64) -> String {
    format_size(bytes, DECIMAL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_get_home_dir() {
        let home = get_home_dir();
        assert!(home.is_absolute(), "主目录应为绝对路径");
    }

    #[test]
    fn test_get_cache_paths_contains_library_caches() {
        let paths = get_cache_paths();
        let home = get_home_dir();
        assert!(
            paths.contains(&home.join("Library/Caches")),
            "缓存路径应包含 ~/Library/Caches"
        );
    }

    #[test]
    fn test_get_app_support_paths_not_empty() {
        let paths = get_app_support_paths();
        assert!(!paths.is_empty(), "应用支持路径不应为空");
    }

    #[test]
    fn test_check_full_disk_access_no_panic() {
        // 只确保不 panic，不断言结果（依赖实际权限）
        let _ = check_full_disk_access();
    }

    #[test]
    fn test_get_trash_size_no_panic() {
        // 只确保不 panic
        let result = get_trash_size();
        assert!(result.is_ok(), "get_trash_size 不应返回错误: {result:?}");
    }

    #[test]
    fn test_calc_dir_size() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        let mut f = File::create(&file_path).unwrap();
        f.write_all(b"hello world").unwrap();

        let size = calc_dir_size(&dir.path().to_path_buf()).unwrap();
        assert_eq!(size, 11, "目录大小应为 11 字节");
    }

    #[test]
    fn test_fmt_size() {
        assert_eq!(fmt_size(0), "0 B");
        assert_eq!(fmt_size(1000), "1 kB");
        assert_eq!(fmt_size(1_000_000), "1 MB");
        assert_eq!(fmt_size(1_000_000_000), "1 GB");
    }

    #[test]
    fn test_move_to_trash_empty() {
        let result = move_to_trash(&[]);
        assert!(result.is_ok(), "空列表应成功");
    }
}
