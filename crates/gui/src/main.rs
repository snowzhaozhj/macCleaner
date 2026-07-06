// 发布构建下阻止 Windows 额外控制台窗口（macOS 无影响，保留以对齐 Tauri 约定）。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mc_gui_lib::run();
}
