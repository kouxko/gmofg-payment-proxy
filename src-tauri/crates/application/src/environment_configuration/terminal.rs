use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum EnvironmentTerminalResult {
    Committed {
        workspace_id: Uuid,
        revision: u64,
        selected_workspace_id: Option<Uuid>,
        apply_task_id: Option<String>,
        status_code: (),
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    ValidationFailed {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    Stale {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    Cancelled {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    CancelledByShutdown {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    FailedBeforeCommit {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
    RolledBack {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<EnvironmentDiagnostic>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentDiagnostic {
    code: EnvironmentStatusCode,
    field: Option<String>,
    message: String,
    severity: DiagnosticSeverity,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvironmentStatusCode {
    SchemaInvalid,
    UnknownField,
    ForbiddenField,
    DtoLimitExceeded,
    WorkspaceNameEmpty,
    WorkspaceNameCollision,
    ListenerAliasDuplicate,
    ListenerAliasMissing,
    ListenerAliasTypeMismatch,
    ListenerDomainInvalid,
    ExistingRuleIdForbidden,
    ExistingRuleIdUnknown,
    ExistingRuleIdDuplicate,
    ExistingRuleIdWorkspaceMismatch,
    ExistingRuleIdKindMismatch,
    ExistingRuleIdBindingMismatch,
    ExistingRuleIdPackageMismatch,
    ExistingRuleIdSchemaVersionMismatch,
    ExistingRuleIdStageMismatch,
    HttpRuleInvalid,
    ProtocolDocumentRuleInvalid,
    DocumentValueWireInvalid,
    WeakNetworkWireInvalid,
    WeakNetworkValueInvalid,
    MaterialAliasDuplicate,
    MaterialAliasMissing,
    MaterialAliasTypeMismatch,
    MaterialAliasUnused,
    MaterialAliasMultipleConsumersUnsupported,
    UnsupportedSecretRole,
    UnsupportedMaterialRole,
    CertificateParseFailed,
    CertificateRoleMismatch,
    SecretValueInvalid,
    InvalidProtocolPackageVersion,
    ProtocolPackageNotInstalled,
    ProtocolPackageDisabled,
    ExternalPackageOffline,
    ProtocolPackageIncompatible,
    McpCreateDeadlineExceeded,
    ValidationLayerFailed,
    CandidateNotFound,
    CandidateStale,
    CandidateCancelled,
    CandidateCancelledByShutdown,
    CandidateCapacityExceeded,
    TargetCandidateAlreadyActive,
    ApplyAlreadyActive,
    ConfirmationTokenMissing,
    ConfirmationTokenInvalid,
    TokenConsumed,
    ShutdownInProgress,
    RuntimeActive,
    AndroidRuntimeOwnerActive,
    AffectedResourceChanged,
    AffectedResourceRemoved,
    ApplyLeaseUnavailable,
    ApplyLeaseMismatch,
    ProtectedMaterialPrepareFailed,
    CommitBaselineMismatch,
    CommitRolledBack,
    CommitFailed,
    HardKillStatusUnavailable,
    Ipv4BindFailed,
    HttpMethodNotAllowed,
    HttpPathNotFound,
    HttpBodyTooLarge,
    HttpMalformed,
    McpProtocolInvalid,
    McpToolArgumentsInvalid,
}
