//! MCP backend over the application facade plus bounded runtime-log and Exchange-observation stores.

mod dispatch;
mod guidance;

use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    AppError, AppErrorViewModel, Application, BreakpointId, ExchangeObservationQueries,
    ProtocolPackageRef, RuleId, RuntimeEpoch, SessionId, WorkspaceId,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{query, resources, server::McpTransportCapabilities};
use crate::runtime_logs::RuntimeLogStore;
use guidance::diagnostic_guidance;

#[derive(Debug, Clone)]
pub struct ToolFailure {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
}

impl ToolFailure {
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_ARGUMENTS".to_owned(),
            message: message.into(),
            details: None,
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "NOT_FOUND".to_owned(),
            message: message.into(),
            details: None,
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "INTERNAL_ERROR".to_owned(),
            message: message.into(),
            details: None,
        }
    }

    pub fn as_value(&self) -> Value {
        json!({
            "code": self.code,
            "message": self.message,
            "details": self.details,
        })
    }
}

impl From<AppError> for ToolFailure {
    fn from(error: AppError) -> Self {
        let view_model = AppErrorViewModel::from(error);
        let code = view_model.code.clone();
        let message = view_model.message.clone();
        let details = json!({
            "code": view_model.code,
            "message": view_model.message,
            "field_errors": view_model.field_errors,
            "retryable": view_model.retryable,
            "suggested_action": view_model.suggested_action,
            "entity_id": view_model.entity_id,
            "runtime_epoch": view_model.runtime_epoch,
            "diagnostic": view_model.diagnostic,
        });
        Self {
            code,
            message,
            details: Some(details),
        }
    }
}

pub type ToolResult = Result<Value, ToolFailure>;

pub(super) const DISPATCHED_TOOL_NAMES: &[&str] = &[
    "application_snapshot",
    "application_log_query",
    "application_log_get",
    "exchange_observation_query",
    "exchange_observation_get",
    "reproduction_report",
    "settings_get",
    "workspace_list",
    "workspace_get",
    "entry_overview",
    "entry_status_list",
    "diagnostics_query",
    "diagnose_recent_failures",
    "external_package_service_status",
    "android_adb_get",
    "android_device_list",
    "android_package_list",
    "android_package_get",
    "android_profile_list",
    "android_profile_get",
    "android_network_status",
    "android_runtime_owner_list",
    "android_network_endpoints",
    "certificate_overview",
    "workspace_certificate_overview",
    "http_capture_query",
    "http_capture_get",
    "breakpoint_query",
    "breakpoint_get",
    "http_rule_list",
    "http_rule_get",
    "protocol_rule_list",
    "workspace_protocol_rule_list",
    "protocol_package_list",
    "protocol_package_catalog",
    "protocol_package_detail",
    "protocol_package_usage",
    super::environment_contract::ENVIRONMENT_TOOL_NAMES[0],
    super::environment_contract::ENVIRONMENT_TOOL_NAMES[1],
    super::environment_contract::ENVIRONMENT_TOOL_NAMES[2],
    super::environment_contract::ENVIRONMENT_TOOL_NAMES[3],
    super::environment_contract::ENVIRONMENT_TOOL_NAMES[4],
];

#[derive(Clone, Debug)]
pub(crate) struct McpCallContext {
    pub request_cancellation: CancellationToken,
    pub transport_capabilities: Arc<McpTransportCapabilities>,
}

pub(crate) enum EnvironmentToolRequest {
    Capabilities {
        transport_capabilities: Arc<McpTransportCapabilities>,
    },
    Create {
        candidate: intercept_proxy_application::EnvironmentConfigurationCandidateV1,
        // Application::environment_candidate_create owns environment_candidate_run_validation;
        // this request-scoped token is forwarded only through that create path.
        request_cancellation: CancellationToken,
    },
    Status {
        candidate_id: intercept_proxy_application::EnvironmentCandidateId,
    },
    Cancel {
        candidate_id: intercept_proxy_application::EnvironmentCandidateId,
    },
    Apply {
        candidate_id: intercept_proxy_application::EnvironmentCandidateId,
        confirmation_token: intercept_proxy_application::EnvironmentConfirmationToken,
    },
}

#[async_trait]
pub trait ReadOnlyMcpBackend: Debug + Send + Sync {
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult;
    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        _context: McpCallContext,
    ) -> ToolResult {
        self.call_tool(name, arguments).await
    }
    async fn read_resource(&self, uri: &str) -> ToolResult;
}

#[derive(Debug)]
pub struct ApplicationBackend {
    application: Arc<Application>,
    runtime_logs: Arc<RuntimeLogStore>,
    exchange_observations: ExchangeObservationQueries,
}

impl ApplicationBackend {
    pub(crate) fn new(
        application: Arc<Application>,
        runtime_logs: Arc<RuntimeLogStore>,
        exchange_observations: impl Into<ExchangeObservationQueries>,
    ) -> Self {
        Self {
            application,
            runtime_logs,
            exchange_observations: exchange_observations.into(),
        }
    }

    async fn application_snapshot(&self) -> ToolResult {
        let snapshot = self.application.application_snapshot().await?;
        let generation = snapshot.generation.clone();
        let observed_at = snapshot.observed_at;
        Ok(json!({
            "snapshot": snapshot,
            "consistency": {
                "strategy": "application_mutation_gate",
                "attempt": 1,
                "generation": generation,
                "observed_at": observed_at,
                "status": "captured_once_under_mutation_gate"
            }
        }))
    }

    fn diagnose_recent_failures(&self, arguments: Value) -> ToolResult {
        let arguments: query::DiagnosticArguments = parse(arguments)?;
        let page = self
            .application
            .diagnostic_log_query(&arguments.into_query());
        let suggestions = page
            .rows
            .iter()
            .map(|row| {
                let evidence = format!(
                    "{} {} {}",
                    row.stage_text,
                    row.summary,
                    row.detail.as_deref().unwrap_or_default()
                );
                let normalized = evidence.to_lowercase();
                let guidance = diagnostic_guidance(&normalized);
                json!({
                    "diagnostic_event_id": row.event_id,
                    "category": guidance.category,
                    "evidence": row,
                    "suggested_ui_path": guidance.ui_path,
                    "suggested_action": guidance.action,
                    "suggested_app_action": guidance.app_action,
                    "alternative_approaches": guidance.alternatives,
                    "risk": "尚未执行；修改配置前记录原值以便回退。",
                    "expected_verification": guidance.verification,
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "diagnostics": page,
            "suggestions": suggestions,
            "execution_state": "suggestions_only_no_application_changes",
        }))
    }
}

#[async_trait]
impl ReadOnlyMcpBackend for ApplicationBackend {
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult {
        if !DISPATCHED_TOOL_NAMES.contains(&name) {
            return unknown_tool(name);
        }
        match name {
            "application_snapshot"
            | "application_log_query"
            | "application_log_get"
            | "exchange_observation_query"
            | "exchange_observation_get"
            | "reproduction_report"
            | "settings_get"
            | "workspace_list"
            | "workspace_get"
            | "entry_overview"
            | "entry_status_list"
            | "diagnostics_query"
            | "diagnose_recent_failures" => self.call_general_tool(name, arguments).await,
            "http_capture_query" | "http_capture_get" | "breakpoint_query" | "breakpoint_get" => {
                self.call_traffic_tool(name, arguments).await
            }
            "http_rule_list"
            | "http_rule_get"
            | "protocol_rule_list"
            | "workspace_protocol_rule_list"
            | "protocol_package_list"
            | "protocol_package_catalog"
            | "protocol_package_detail"
            | "protocol_package_usage" => self.call_configuration_tool(name, arguments).await,
            "android_adb_get"
            | "external_package_service_status"
            | "android_device_list"
            | "android_package_list"
            | "android_package_get"
            | "android_profile_list"
            | "android_profile_get"
            | "android_network_status"
            | "android_runtime_owner_list"
            | "android_network_endpoints"
            | "certificate_overview"
            | "workspace_certificate_overview" => self.call_runtime_tool(name, arguments).await,
            _ => unknown_tool(name),
        }
    }

    async fn call_tool_with_context(
        &self,
        name: &str,
        arguments: Value,
        context: McpCallContext,
    ) -> ToolResult {
        if let Some(kind) = super::environment_contract::environment_tool_kind(name) {
            let request = Self::environment_tool_request(kind, arguments, context)?;
            return self.call_environment_tool(request).await;
        }
        self.call_tool(name, arguments).await
    }

    async fn read_resource(&self, uri: &str) -> ToolResult {
        if let Some((mime_type, text)) = resources::text(uri) {
            return Ok(json!({ "uri": uri, "mimeType": mime_type, "text": text }));
        }
        if uri == resources::ISO8583_ARCHIVE_URI {
            let bytes = self.application.protocol_package_builtin_archive().await?;
            return Ok(json!({
                "uri": uri,
                "mimeType": "application/zip",
                "blob": STANDARD.encode(bytes),
            }));
        }
        Err(ToolFailure::not_found(format!(
            "Unknown resource URI: {uri}"
        )))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceArguments {
    workspace_id: WorkspaceId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationLogDetailArguments {
    log_id: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpCaptureDetailArguments {
    session_id: SessionId,
    runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpRuleArguments {
    rule_id: RuleId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolPackageArguments {
    package: ProtocolPackageRef,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct BreakpointQueryArguments {
    runtime_epoch: Option<RuntimeEpoch>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BreakpointDetailArguments {
    breakpoint_id: BreakpointId,
    runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidPackageArguments {
    serial: String,
    package_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidDeviceArguments {
    serial: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidProfileArguments {
    profile_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AndroidEndpointArguments {
    serial: String,
    profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentCreateArguments {
    candidate: intercept_proxy_application::EnvironmentConfigurationCandidateV1,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentCandidateArguments {
    candidate_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentApplyArguments {
    candidate_id: String,
    confirmation_token: String,
}

fn parse<T: DeserializeOwned>(arguments: Value) -> Result<T, ToolFailure> {
    let arguments = if arguments.is_null() {
        json!({})
    } else {
        arguments
    };
    serde_json::from_value(arguments)
        .map_err(|error| ToolFailure::invalid_arguments(error.to_string()))
}

fn json_value(value: impl serde::Serialize) -> ToolResult {
    serde_json::to_value(value).map_err(|error| ToolFailure::internal(error.to_string()))
}

fn unknown_tool(name: &str) -> ToolResult {
    Err(ToolFailure::not_found(format!("Unknown tool: {name}")))
}
