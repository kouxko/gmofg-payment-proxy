//! 通用拦截代理的操作系统与持久化适配层。
//!
//! 本 crate 保存配置和“已加密”的证书材料，并实现显式导入导出；抓到的 HTTP
//! 请求/响应正文只属于内存会话，不在这里落盘。上层只依赖应用端口，因此 `SQLite`、
//! DPAPI/Keychain 或证书库失败时可以被统一映射，而不会污染领域模型。

mod adapters;
pub mod application_backup;
mod application_backup_export;
mod application_backup_import;
pub mod certificates;
pub mod dpapi;
pub mod error;
pub mod files;
#[cfg(target_os = "macos")]
pub mod keychain;
pub mod sqlite;
mod windows_process;

pub use adapters::{
    EnvironmentApplyLeaseAdapter, EnvironmentConfigurationMaterialPreparer,
    ExchangeObservationCounters, ExchangeObservationStore, ExternalPackageServer, FileSelection,
    InfrastructureServiceBundle, NativeFileDialog,
};
pub use application_backup::{
    ApplicationBackupArchive, ApplicationBackupArchiveError, ApplicationBackupArchiveErrorCode,
    ApplicationBackupArchiveLimits, DEFAULT_MAX_APPLICATION_BACKUP_ARCHIVE_BYTES,
};
pub use application_backup_export::{ApplicationBackupFileExporter, build_application_backup_zip};
pub use application_backup_import::{
    ApplicationBackupImportClock, ApplicationBackupImportPreparer,
    ApplicationBackupImportTokenGenerator, DEFAULT_APPLICATION_BACKUP_PENDING_BYTES,
    DEFAULT_APPLICATION_BACKUP_PENDING_CAPACITY, DEFAULT_APPLICATION_BACKUP_PENDING_TTL,
    RandomApplicationBackupImportTokenGenerator, SystemApplicationBackupImportClock,
};
pub use certificates::{
    CertificateBundle, CertificateMetadata, CertificateService, LeafCertificateRequest,
    ParsedPkcs12,
};
pub use dpapi::{DpapiProtector, SecretProtector};
pub use error::{InfrastructureError, InfrastructureErrorCode};
pub use files::{
    ApplicationBackupFileSystem, ApplicationBackupTemporaryFile, AtomicFileExporter, ExportOutcome,
    SystemApplicationBackupFileSystem,
};
#[cfg(target_os = "macos")]
pub use keychain::MacKeychainProtector;
pub use sqlite::{
    AndroidRuntimeOwnerRecord, CURRENT_APPLICATION_SCHEMA_VERSION, CertificateMaterialRecord,
    CertificateMaterialSnapshot, EnvironmentCommitFaultPoint, IntoSqlitePersistence,
    ProtectedSecretRecord, SqliteExecutor, SqliteStore, StoredSettings,
    WorkspaceCollectionSnapshot, WorkspaceRecord, open_sqlite_persistence,
};
