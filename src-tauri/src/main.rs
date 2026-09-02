//! 桌面应用的最小可执行入口；所有装配与退出控制均位于库 crate。

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    intercept_proxy::run();
}
