use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Serialize)]
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

#[derive(Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
enum EnvironmentTerminalResultWire {
    Committed {
        workspace_id: Uuid,
        revision: u64,
        selected_workspace_id: Option<Uuid>,
        apply_task_id: Option<String>,
        status_code: (),
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    ValidationFailed {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    Stale {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    Cancelled {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    CancelledByShutdown {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    FailedBeforeCommit {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
    RolledBack {
        status_code: EnvironmentStatusCode,
        diagnostics: Vec<serde::de::IgnoredAny>,
    },
}

impl<'de> Deserialize<'de> for EnvironmentTerminalResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = EnvironmentTerminalResultWire::deserialize(deserializer)?;
        wire.try_into().map_err(serde::de::Error::custom)
    }
}

impl TryFrom<EnvironmentTerminalResultWire> for EnvironmentTerminalResult {
    type Error = &'static str;

    fn try_from(wire: EnvironmentTerminalResultWire) -> Result<Self, Self::Error> {
        match wire {
            EnvironmentTerminalResultWire::Committed {
                workspace_id,
                revision,
                selected_workspace_id,
                apply_task_id,
                status_code,
                diagnostics,
            } => {
                require_empty_diagnostics(&diagnostics)?;
                Ok(Self::Committed {
                    workspace_id,
                    revision,
                    selected_workspace_id,
                    apply_task_id,
                    status_code,
                    diagnostics: Vec::new(),
                })
            }
            EnvironmentTerminalResultWire::ValidationFailed {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::ValidationFailed {
                status_code,
                diagnostics: Vec::new(),
            }),
            EnvironmentTerminalResultWire::Stale {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::Stale {
                status_code,
                diagnostics: Vec::new(),
            }),
            EnvironmentTerminalResultWire::Cancelled {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::Cancelled {
                status_code,
                diagnostics: Vec::new(),
            }),
            EnvironmentTerminalResultWire::CancelledByShutdown {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::CancelledByShutdown {
                status_code,
                diagnostics: Vec::new(),
            }),
            EnvironmentTerminalResultWire::FailedBeforeCommit {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::FailedBeforeCommit {
                status_code,
                diagnostics: Vec::new(),
            }),
            EnvironmentTerminalResultWire::RolledBack {
                status_code,
                diagnostics,
            } => terminal_without_diagnostics(&diagnostics, || Self::RolledBack {
                status_code,
                diagnostics: Vec::new(),
            }),
        }
    }
}

fn terminal_without_diagnostics(
    diagnostics: &[serde::de::IgnoredAny],
    create: impl FnOnce() -> EnvironmentTerminalResult,
) -> Result<EnvironmentTerminalResult, &'static str> {
    require_empty_diagnostics(diagnostics)?;
    Ok(create())
}

fn require_empty_diagnostics(diagnostics: &[serde::de::IgnoredAny]) -> Result<(), &'static str> {
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err("terminal diagnostics are output-only")
    }
}

impl EnvironmentTerminalResult {
    pub(super) fn committed(
        workspace_id: Uuid,
        revision: u64,
        selected_workspace_id: Option<Uuid>,
        apply_task_id: Option<String>,
    ) -> Self {
        Self::Committed {
            workspace_id,
            revision,
            selected_workspace_id,
            apply_task_id,
            status_code: (),
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn failed_before_commit(status_code: EnvironmentStatusCode) -> Self {
        Self::FailedBeforeCommit {
            status_code,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn validation_failed(status_code: EnvironmentStatusCode) -> Self {
        Self::ValidationFailed {
            status_code,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn rolled_back(status_code: EnvironmentStatusCode) -> Self {
        Self::RolledBack {
            status_code,
            diagnostics: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn stale() -> Self {
        Self::Stale {
            status_code: EnvironmentStatusCode::CandidateStale,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn stale_with(status_code: EnvironmentStatusCode) -> Self {
        Self::Stale {
            status_code,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn cancelled() -> Self {
        Self::Cancelled {
            status_code: EnvironmentStatusCode::CandidateCancelled,
            diagnostics: Vec::new(),
        }
    }

    pub(super) fn cancelled_by_shutdown() -> Self {
        Self::CancelledByShutdown {
            status_code: EnvironmentStatusCode::CandidateCancelledByShutdown,
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EnvironmentDiagnostic {
    code: EnvironmentStatusCode,
    field: Option<EnvironmentDiagnosticField>,
    message: &'static str,
    severity: DiagnosticSeverity,
}

impl EnvironmentDiagnostic {
    pub(crate) const fn error(code: EnvironmentStatusCode) -> Self {
        Self {
            code,
            field: None,
            message: code.safe_message(),
            severity: DiagnosticSeverity::Error,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct EnvironmentDiagnosticField {
    scope: EnvironmentDiagnosticScope,
    index: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvironmentDiagnosticScope {
    Candidate,
    Target,
    Workspace,
    Listener,
    HttpRule,
    ProtocolRule,
    Material,
    AndroidProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
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

impl EnvironmentStatusCode {
    const fn safe_message(self) -> &'static str {
        match self {
            Self::CandidateNotFound => "environment candidate was not found",
            Self::ValidationLayerFailed => "environment validation layer failed",
            Self::CandidateStale => "environment candidate is stale",
            Self::CandidateCancelled => "environment candidate was cancelled",
            Self::CandidateCancelledByShutdown => {
                "environment candidate was cancelled during shutdown"
            }
            Self::CommitRolledBack => "environment configuration commit was rolled back",
            Self::CommitFailed => "environment configuration commit failed",
            _ => "environment configuration operation failed",
        }
    }
}
