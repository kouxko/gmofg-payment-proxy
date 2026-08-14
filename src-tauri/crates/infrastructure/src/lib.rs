//! 通用拦截代理的操作系统与持久化适配层。
//!
//! 本 crate 保存配置、规则和“已加密”的证书材料，并实现显式导入导出；抓到的 HTTP
//! 请求/响应正文只属于内存会话，不在这里落盘。上层只依赖应用端口，因此 `SQLite`、
//! DPAPI/Keychain 或证书库失败时可以被统一映射，而不会污染领域模型。

pub mod adapters;
pub mod certificates;
pub mod dpapi;
pub mod error;
pub mod files;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod sqlite;
mod windows_process;

pub use adapters::{
    AndroidAdbAdapter, ApplicationProxyAdapter, CaptureRepositoryAdapter,
    CertificateServiceAdapter, FaultServiceAdapter, HeaderBodyCodecResolver,
    InfrastructureServiceBundle, ListenerRuntimeAdapter, NativeFileDialog, ProtectedSecretAdapter,
    ProtocolPackageImportAdapter, ProtocolPackageInstallOutcome, ProtocolPackageRecoveryFailure,
    ProtocolPackageRecoveryReport, ProtocolPackageRepositoryAdapter, ProtocolPackageStorageError,
    ProtocolPackageStorageErrorCode, ProtocolPackageSummary, ProtocolPackageUsageQueryAdapter,
    ProtocolPackageValidationStatus, RetiredProxyAdapter, RuleRepositoryAdapter,
    RuntimePipelineAdapter, RuntimePipelineProductHooks, RuntimeRuleRepository,
    SettingsRepositoryAdapter, WorkspaceBodyCodecResolver, WorkspaceDocumentAdapter,
    WorkspaceRepositoryAdapter, WorkspaceRuntimePolicyResolver,
};
pub use certificates::{
    CertificateBundle, CertificateMetadata, CertificateService, LeafCertificateRequest,
    ParsedPkcs12,
};
pub use dpapi::{DpapiProtector, SecretProtector};
pub use error::{InfrastructureError, InfrastructureErrorCode};
pub use files::{AtomicFileExporter, ExportOutcome};
#[cfg(target_os = "macos")]
pub use keychain::MacKeychainProtector;
pub use sqlite::{
    CertificateMaterialRecord, CertificateMaterialSnapshot, ProtectedSecretRecord,
    RuleCollectionSnapshot, RuleRecord, SqliteStore, StoredSettings, WorkspaceCollectionSnapshot,
    WorkspaceRecord,
};
