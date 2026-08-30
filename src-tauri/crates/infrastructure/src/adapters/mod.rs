//! 应用端口到数据库、证书、文件系统和代理运行时的适配器集合。
//!
//! 适配器负责 DTO/错误映射与资源组合，不把 Tauri/WebView 类型带入业务层，也不在此处
//! 重新定义领域规则。

mod android_adb;
mod body_codecs;
mod bundle;
mod capture;
mod certificates;
pub(crate) mod common;
#[cfg(test)]
mod environment_apply_lease_tests;
mod environment_apply_resources;
#[cfg(test)]
#[path = "environment_apply_lease_tests/revision16_integration.rs"]
mod environment_apply_revision16_integration;
#[cfg(test)]
mod environment_apply_shared_gate_integration;
mod environment_configuration_baseline_capture;
mod environment_configuration_lease;
mod environment_configuration_materials;
mod environment_configuration_validation;
mod exchange_observation;
mod external_package_registry;
mod external_package_server;
pub mod external_packages;
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
mod rules;
mod settings;
mod workspaces;

pub use crate::package_transport::{
    PackageTransportClient, PackageTransportConfig, PackageTransportError,
};
pub use android_adb::AndroidAdbAdapter;
pub use body_codecs::{HeaderBodyCodecResolver, WorkspaceBodyCodecResolver};
pub use bundle::InfrastructureServiceBundle;
pub use capture::CaptureRepositoryAdapter;
pub use certificates::CertificateServiceAdapter;
pub(crate) use environment_apply_resources::EnvironmentApplyResourceGateRegistry;
pub use environment_configuration_lease::EnvironmentApplyLeaseAdapter;
pub(crate) use environment_configuration_lease::EnvironmentApplyRuntimeAdapter;
#[cfg(test)]
pub(crate) use environment_configuration_lease::{
    EnvironmentApplyLeaseResourceKey, EnvironmentApplyLeaseResourceObservation,
    EnvironmentApplyLeaseRuntime,
};
pub use environment_configuration_materials::EnvironmentConfigurationMaterialPreparer;
pub(crate) use environment_configuration_materials::{
    PreparedMaterialArena, PreparedMaterialBatch, PreparedMaterialRecord,
};
pub(crate) use environment_configuration_validation::EnvironmentConfigurationValidationAdapter;
pub use exchange_observation::{ExchangeObservationCounters, ExchangeObservationStore};
pub use external_package_registry::{
    AcceptedExternalPackageConnection, ExternalPackageConnectionId, ExternalPackageRegistryAdapter,
    external_package_registration_fingerprint,
};
pub use external_package_server::{ExternalPackageServer, ExternalPackageServerConfig};
pub use external_packages::accept_packages_websocket;
pub use faults::FaultServiceAdapter;
pub use files::{FileSelection, NativeFileDialog};
pub use listener_certificates::ManagedListenerCertificateAdapter;
pub use listener_runtime::ListenerRuntimeAdapter;
pub use pipeline::{RuntimePipelineAdapter, RuntimePipelineProductHooks};
pub use protected_secrets::ProtectedSecretAdapter;
pub use protocol_package_import::ProtocolPackageImportAdapter;
pub use protocol_package_usage::ProtocolPackageUsageQueryAdapter;
pub use protocol_packages::ProtocolPackageRepositoryAdapter;
pub use rules::RuleRepositoryAdapter;
pub use settings::SettingsRepositoryAdapter;
pub use workspaces::WorkspaceRepositoryAdapter;
