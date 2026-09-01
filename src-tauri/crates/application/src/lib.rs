//! 应用用例与前端无关的展示模型。
//!
//! 规范化值、中文状态、色调语义、操作权限、分页、校验和稳定错误都由 Rust 负责，
//! Tauri、未来 TUI/CLI 只渲染同一份协议，不重复作业务决定。这些模型不绑定具体组件库。
//!
//! 本 crate 还定义由 infrastructure 适配器实现的端口，但不包含 Tauri、数据库、TLS
//! 或文件系统实现。

mod android;
mod application_backup;
mod backup_export;
mod backup_import;
mod capacity;
mod configuration;
mod document_security;
mod environment_configuration;
mod error;
mod events;
mod facade;
mod listeners;
mod models;
mod portable_certificates;
mod portable_protocol_packages;
mod ports;
mod sessions;
mod workspace_persistence;
mod workspaces;

pub use android::*;
pub use application_backup::*;
pub use backup_export::*;
pub use backup_import::*;
pub use capacity::CapacityLedger;
pub use configuration::*;
pub use environment_configuration::ENVIRONMENT_VALIDATION_ENGINE_VERSION;
#[cfg(test)]
pub(crate) use environment_configuration::EnvironmentIdentityAllocatorPort;
pub use environment_configuration::{
    DiagnosticSeverity, EnvironmentAffectedListenerBaseline, EnvironmentAndroidOwnerBaseline,
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentApplyGenerations, EnvironmentApplyLease, EnvironmentApplyLeaseOutcome,
    EnvironmentApplyLeasePort, EnvironmentApplyLeaseRequest, EnvironmentApplyQueuedResult,
    EnvironmentApplyTaskId, EnvironmentCancelResult, EnvironmentCancelStatus,
    EnvironmentCandidateCreateResult, EnvironmentCandidateEpoch, EnvironmentCandidateId,
    EnvironmentCandidateLifecycleError, EnvironmentCandidateMetrics,
    EnvironmentCandidatePublicSnapshot, EnvironmentCandidateStatus,
    EnvironmentCandidateStatusResult, EnvironmentCandidateValidator, EnvironmentCommitFailure,
    EnvironmentCommitPort, EnvironmentCommitReceipt, EnvironmentCommitRequest,
    EnvironmentCommitResult, EnvironmentCommitRollbackOutcome, EnvironmentCommitTarget,
    EnvironmentConfigurationCandidateV1, EnvironmentConfirmationToken,
    EnvironmentConsumedCommitRequest, EnvironmentConsumedPreparedMaterials, EnvironmentDiagnostic,
    EnvironmentDiagnosticScope, EnvironmentDnsTcpTarget, EnvironmentExactPackageBaseline,
    EnvironmentIdentityAllocator, EnvironmentMaterialInventoryBaseline, EnvironmentMaterialProbe,
    EnvironmentMaterialProbeKind, EnvironmentPreparedMaterialCapability,
    EnvironmentPreparedMaterialKind, EnvironmentPreparedMaterialVisitor,
    EnvironmentPreparedMaterials, EnvironmentProtectedMaterialPreparePort,
    EnvironmentSelectionPolicy, EnvironmentStatusCode, EnvironmentTerminalResult,
    EnvironmentTlsMtlsTarget, EnvironmentValidatedApplyBaseline,
    EnvironmentValidatedApplyBaselineCollector, EnvironmentValidationLayer,
    EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest,
    EnvironmentValidationLayerResult, EnvironmentValidationReport, EnvironmentValidationResult,
    EnvironmentValidationStatus, MaterialAlias, StagedProtectedMaterialHandle,
    parse_environment_configuration_candidate_v1,
};
#[cfg(test)]
pub(crate) use environment_configuration::{
    EnvironmentApplyWork, EnvironmentCandidatePolicy, EnvironmentCandidateRegistry,
};
#[cfg(test)]
#[allow(
    unused_imports,
    reason = "crate-internal validation contract is consumed by requirement tests"
)]
pub(crate) use environment_configuration::{
    EnvironmentPreviewBaselinePort, EnvironmentPreviewBaselineRequest,
};
pub use error::{AppError, AppErrorDiagnosticViewModel, AppErrorViewModel, AppResult};
pub use events::{EventHub, EventReplay, EventSubscription};
pub use facade::{
    Application, ApplicationDependencies, EnvironmentConfigurationApplicationServices,
    ExchangeObservationQueries, parse_protocol_rule_value, validate_portable_protocol_bindings,
};
pub use listeners::InMemoryListenerRuntime;
pub use models::*;
pub use portable_certificates::*;
pub use portable_protocol_packages::*;
pub use ports::*;
pub use sessions::{InMemorySessionStore, SessionStore};
pub use workspace_persistence::WORKSPACE_PERSISTENCE_VERSION;
pub use workspaces::{InMemoryWorkspaceStore, remap_workspace_identity};

#[cfg(test)]
mod requirements_tests;
