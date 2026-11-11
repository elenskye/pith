// 正式构建不弹终端窗口（仅 Windows 有意义，保留 Tauri 惯例）
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    pith_lib::run()
}
