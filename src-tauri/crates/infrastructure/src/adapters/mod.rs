//! 应用端口到数据库、证书、文件系统和代理运行时的适配器集合。
//!
//! 适配器负责 DTO/错误映射与资源组合，不把 Tauri/WebView 类型带入业务层，也不在此处
//! 重新定义领域规则。

mod application_proxy;
mod bundle;
mod capture;
mod certificates;
mod common;
mod faults;
mod files;
mod pipeline;
mod rules;
mod settings;

pub use application_proxy::ApplicationProxyAdapter;
pub use bundle::InfrastructureServiceBundle;
pub use capture::CaptureRepositoryAdapter;
pub use certificates::CertificateServiceAdapter;
pub use faults::FaultServiceAdapter;
pub use files::{FileExportAdapter, FileSelection, NativeFileDialog};
pub use pipeline::{RuntimePipelineAdapter, RuntimePipelineProductHooks, RuntimeRuleRepository};
pub use rules::RuleRepositoryAdapter;
pub use settings::SettingsRepositoryAdapter;
