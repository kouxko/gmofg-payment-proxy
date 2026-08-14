//! 应用端口到数据库、证书、文件系统和代理运行时的适配器集合。
//!
//! 适配器负责 DTO/错误映射与资源组合，不把 Tauri/WebView 类型带入业务层，也不在此处
//! 重新定义领域规则。

mod android_adb;
mod application_proxy;
mod body_codecs;
mod bundle;
mod capture;
mod certificates;
mod common;
mod faults;
mod files;
mod listener_certificate_metadata;
mod listener_certificate_store;
mod listener_certificates;
mod listener_runtime;
mod pipeline;
mod protected_secrets;
mod protocol_package_import;
mod protocol_package_usage;
mod protocol_packages;
mod retired_proxy;
mod rules;
mod settings;
mod workspace_policies;
mod workspaces;

pub use android_adb::AndroidAdbAdapter;
pub use application_proxy::ApplicationProxyAdapter;
pub use body_codecs::{HeaderBodyCodecResolver, WorkspaceBodyCodecResolver};
pub use bundle::InfrastructureServiceBundle;
pub use capture::CaptureRepositoryAdapter;
pub use certificates::CertificateServiceAdapter;
pub use faults::FaultServiceAdapter;
pub use files::{FileSelection, NativeFileDialog};
pub use listener_certificates::ManagedListenerCertificateAdapter;
pub use listener_runtime::ListenerRuntimeAdapter;
pub use pipeline::{
    RuntimeBodyCodecResolver, RuntimePipelineAdapter, RuntimePipelineProductHooks,
    RuntimeRuleRepository,
};
pub use protected_secrets::ProtectedSecretAdapter;
pub use protocol_package_import::ProtocolPackageImportAdapter;
pub use protocol_package_usage::ProtocolPackageUsageQueryAdapter;
use protocol_packages::PreparedProtocolPackage;
pub use protocol_packages::{
    ProtocolPackageInstallOutcome, ProtocolPackageRecoveryFailure, ProtocolPackageRecoveryReport,
    ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError, ProtocolPackageStorageErrorCode,
    ProtocolPackageSummary, ProtocolPackageValidationStatus,
};
pub use retired_proxy::RetiredProxyAdapter;
pub use rules::RuleRepositoryAdapter;
pub use settings::SettingsRepositoryAdapter;
pub use workspace_policies::WorkspaceRuntimePolicyResolver;
pub use workspaces::{WorkspaceDocumentAdapter, WorkspaceRepositoryAdapter};
