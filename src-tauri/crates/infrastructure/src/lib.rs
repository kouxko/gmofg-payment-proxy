//! Operating-system and persistence adapters for the payment proxy.
//!
//! This crate never persists captured HTTP payloads. It only owns durable
//! configuration, encrypted certificate material, certificate construction,
//! and explicit user-requested exports.

pub mod adapters;
pub mod certificates;
pub mod dpapi;
pub mod error;
pub mod files;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod sqlite;

pub use adapters::{
    ApplicationProxyAdapter, CaptureRepositoryAdapter, CertificateServiceAdapter,
    FaultServiceAdapter, FileExportAdapter, InfrastructureServiceBundle, NativeFileDialog,
    RuleRepositoryAdapter, RuntimePipelineAdapter, RuntimeRuleRepository,
    SettingsRepositoryAdapter,
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
    CertificateMaterialRecord, CertificateMaterialSnapshot, RuleCollectionSnapshot, RuleRecord,
    SqliteStore, StoredSettings,
};
