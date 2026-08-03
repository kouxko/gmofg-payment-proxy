//! Android 应用网络接管的数据面核心。
//!
//! 本 crate 不依赖 Android Framework 或 Tauri。Android/Kotlin 只负责取得 TUN、
//! 验证系统授权及维护服务生命周期；包选择规则、弱网决策和统计都可以在桌面主机上
//! 直接执行单元测试。未来 TUI/CLI 也可以复用同一组 [`NetworkProfile`] 与
//! [`ImpairmentEngine`]。

mod engine;
mod model;
mod rng;
#[cfg(any(target_os = "android", all(test, unix)))]
mod routing;
mod validation;

#[cfg(any(target_os = "android", all(test, unix)))]
mod data_plane;

#[cfg(any(target_os = "android", all(test, unix)))]
mod jni_bridge;

pub use engine::*;
pub use model::*;
pub use validation::*;

/// 固定的 Android Companion 包名。
///
/// Companion 本身绝不能进入允许列表，否则它创建的转发连接可能再次回到 TUN，
/// 形成递归路由。
pub const COMPANION_PACKAGE_NAME: &str = "com.interceptproxy.vpn";

/// 单个弱网 Profile 最多接管的应用数。
pub const MAX_TARGET_APPLICATIONS: usize = 64;
