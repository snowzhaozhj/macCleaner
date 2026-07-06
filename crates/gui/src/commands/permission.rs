//! FDA（Full Disk Access）权限命令。FDA 无法编程申请（TCC 不弹框、非公开 API，KTD-7），
//! 只能探针检测 + 引导用户去系统设置手动授权。

use mc_core::doctor::{probe_all, standard_fda_paths, PathStatus, ProbeResult};
use serde::Serialize;

/// 一键跳转「系统设置 › 隐私与安全性 › 完全磁盘访问权限」的 URL scheme（KTD-7）。
pub const FDA_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

/// FDA 检测结果：是否已授权 + 每条受保护路径的探针明细（供前端明示被跳过路径，R10）。
#[derive(Debug, Serialize)]
pub struct FdaStatus {
    pub authorized: bool,
    pub probes: Vec<ProbeResult>,
}

/// 从探针明细判定是否已授权：任一受保护路径 `NoPermission` 即未授权（纯函数便于单测）。
/// `Missing` 不算未授权（该路径在此机器上本就不存在，非权限问题）。
pub fn evaluate(probes: Vec<ProbeResult>) -> FdaStatus {
    let authorized = !probes.iter().any(|p| p.status == PathStatus::NoPermission);
    FdaStatus { authorized, probes }
}

/// 检测 FDA：探测标准受保护路径集，返回授权状态 + 明细。
/// `read_dir` 是阻塞 IO，放 `spawn_blocking`（路径少但仍不占主线程）。
#[tauri::command]
pub async fn check_fda() -> Result<FdaStatus, String> {
    tauri::async_runtime::spawn_blocking(|| evaluate(probe_all(&standard_fda_paths())))
        .await
        .map_err(|e| format!("权限检测线程异常: {e}"))
}

/// 引导用户到系统设置的 FDA 面板。授权后通常需重启 app 才生效（前端提示）。
#[tauri::command]
pub async fn open_fda_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url(FDA_SETTINGS_URL, None::<&str>)
        .map_err(|e| format!("打开系统设置失败: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn probe(path: &str, status: PathStatus) -> ProbeResult {
        ProbeResult { path: PathBuf::from(path), status }
    }

    #[test]
    fn no_permission_means_unauthorized() {
        let status = evaluate(vec![
            probe("/a", PathStatus::Readable),
            probe("/b", PathStatus::NoPermission),
        ]);
        assert!(!status.authorized, "存在 NoPermission → 未授权");
    }

    #[test]
    fn all_readable_means_authorized() {
        let status = evaluate(vec![
            probe("/a", PathStatus::Readable),
            probe("/b", PathStatus::Readable),
        ]);
        assert!(status.authorized);
    }

    #[test]
    fn missing_paths_do_not_block_authorization() {
        // Missing = 该保护路径此机器不存在，非权限问题，不应判为未授权。
        let status = evaluate(vec![
            probe("/a", PathStatus::Readable),
            probe("/b", PathStatus::Missing),
        ]);
        assert!(status.authorized, "仅 Missing 不应判未授权");
    }
}
