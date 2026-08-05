//! UI 无关 `ApplicationHost` 的产品适配矩阵测试。
//!
//! 这里用测试 `ProductProfile` 和平台服务从组合根建立真实 `Application`，证明核心可被
//! Tauri 之外的入口复用。它验证 Command/use case 语义，但不打开 WebView，也不声称
//! 已完成 Android 真机或 GMO-FG 上游验收。

include!("application_matrix/support.rs");
include!("application_matrix/validation_and_queries.rs");
include!("application_matrix/rules_and_faults.rs");
include!("application_matrix/certificates.rs");
include!("application_matrix/lifecycle.rs");
