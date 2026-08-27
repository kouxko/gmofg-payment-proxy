//! Tool dispatch groups kept separate from transport and protocol handling.

use serde_json::Value;

use super::{
    AndroidDeviceArguments, AndroidEndpointArguments, AndroidPackageArguments,
    AndroidProfileArguments, ApplicationBackend, ApplicationLogDetailArguments,
    BreakpointDetailArguments, BreakpointQueryArguments, EnvironmentApplyArguments,
    EnvironmentCandidateArguments, EnvironmentCreateArguments, EnvironmentToolRequest,
    HttpCaptureDetailArguments, HttpRuleArguments, McpCallContext, ProtocolPackageArguments,
    ToolFailure, ToolResult, WorkspaceArguments, json_value, parse, query, unknown_tool,
};
use crate::{reproduction_report, runtime_logs::ApplicationLogQuery};
use intercept_proxy_application::{
    EnvironmentCandidateId, EnvironmentCandidateLifecycleError, EnvironmentConfirmationToken,
    ExchangeObservationQuery,
};

use crate::mcp::environment_contract::{
    EnvironmentIpBindingProjection, EnvironmentToolKind, EnvironmentTransportProjection,
    environment_capabilities_output,
};

impl ApplicationBackend {
    pub(super) fn environment_tool_request(
        kind: EnvironmentToolKind,
        arguments: Value,
        context: McpCallContext,
    ) -> Result<EnvironmentToolRequest, ToolFailure> {
        match kind {
            EnvironmentToolKind::Capabilities => Ok(EnvironmentToolRequest::Capabilities {
                transport_capabilities: context.transport_capabilities,
            }),
            EnvironmentToolKind::Create => {
                let arguments: EnvironmentCreateArguments = parse_environment(arguments)?;
                Ok(EnvironmentToolRequest::Create {
                    candidate: arguments.candidate,
                    request_cancellation: context.request_cancellation,
                })
            }
            EnvironmentToolKind::Status => {
                let arguments: EnvironmentCandidateArguments = parse_environment(arguments)?;
                Ok(EnvironmentToolRequest::Status {
                    candidate_id: environment_candidate_id(arguments.candidate_id)?,
                })
            }
            EnvironmentToolKind::Cancel => {
                let arguments: EnvironmentCandidateArguments = parse_environment(arguments)?;
                Ok(EnvironmentToolRequest::Cancel {
                    candidate_id: environment_candidate_id(arguments.candidate_id)?,
                })
            }
            EnvironmentToolKind::Apply => {
                let arguments: EnvironmentApplyArguments = parse_environment(arguments)?;
                Ok(EnvironmentToolRequest::Apply {
                    candidate_id: environment_candidate_id(arguments.candidate_id)?,
                    confirmation_token: EnvironmentConfirmationToken::new(
                        arguments.confirmation_token,
                    )
                    .map_err(|_| environment_arguments_failure())?,
                })
            }
        }
    }

    pub(super) async fn call_environment_tool(
        &self,
        request: EnvironmentToolRequest,
    ) -> ToolResult {
        match request {
            EnvironmentToolRequest::Capabilities {
                transport_capabilities,
            } => Ok(environment_capabilities_output(transport_projection(
                &transport_capabilities,
            ))),
            EnvironmentToolRequest::Create {
                candidate,
                request_cancellation,
            } => json_value(
                self.application
                    .environment_candidate_create(candidate, request_cancellation)
                    .await
                    .map_err(|error| environment_lifecycle_failure(&error))?,
            ),
            EnvironmentToolRequest::Status { candidate_id } => {
                json_value(self.application.environment_candidate_status(&candidate_id))
            }
            EnvironmentToolRequest::Cancel { candidate_id } => {
                json_value(self.application.environment_candidate_cancel(&candidate_id))
            }
            EnvironmentToolRequest::Apply {
                candidate_id,
                confirmation_token,
            } => json_value(
                self.application
                    .environment_candidate_queue_and_start_apply(&candidate_id, &confirmation_token)
                    .map_err(|error| environment_lifecycle_failure(&error))?,
            ),
        }
    }

    pub(super) async fn call_general_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "application_snapshot" => self.application_snapshot().await,
            "application_log_query" => {
                let query: ApplicationLogQuery = parse(arguments)?;
                json_value(self.runtime_logs.query(&query))
            }
            "application_log_get" => {
                let args: ApplicationLogDetailArguments = parse(arguments)?;
                let entry = self.runtime_logs.get(args.log_id).ok_or_else(|| {
                    ToolFailure::not_found(format!(
                        "Application log {} is outside the retained range.",
                        args.log_id
                    ))
                })?;
                json_value(entry)
            }
            "exchange_observation_query" => {
                let query: ExchangeObservationQuery = parse(arguments)?;
                json_value(self.exchange_observations.query(&query))
            }
            "exchange_observation_get" => {
                let exchange_id = arguments
                    .get("exchange_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ToolFailure::invalid_arguments("exchange_id is required"))?;
                let record = self.exchange_observations.get(exchange_id).ok_or_else(|| {
                    ToolFailure::not_found(format!(
                        "Exchange {exchange_id} is outside the retained range."
                    ))
                })?;
                json_value(record)
            }
            "reproduction_report" => {
                let query: intercept_proxy_application::DiagnosticReportQuery = parse(arguments)?;
                let report =
                    reproduction_report::generate(&self.application, &self.runtime_logs, query)
                        .await?;
                Ok(serde_json::json!({
                    "bundle": report.application.bundle,
                    "application_logs": report.application_logs,
                    "markdown": report.markdown,
                }))
            }
            "settings_get" => json_value(self.application.settings_get().await?),
            "workspace_list" => json_value(self.application.workspace_list().await?),
            "workspace_get" => {
                let args: WorkspaceArguments = parse(arguments)?;
                json_value(self.application.workspace_get(args.workspace_id).await?)
            }
            "entry_overview" => {
                let args: WorkspaceArguments = parse(arguments)?;
                json_value(
                    self.application
                        .listener_overview(args.workspace_id)
                        .await?,
                )
            }
            "entry_status_list" => json_value(self.application.listener_statuses().await?),
            "diagnostics_query" => {
                let args: query::DiagnosticArguments = parse(arguments)?;
                json_value(self.application.diagnostic_log_query(&args.into_query()))
            }
            "diagnose_recent_failures" => self.diagnose_recent_failures(arguments),
            _ => unknown_tool(name),
        }
    }

    pub(super) async fn call_traffic_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "http_capture_query" => {
                let args: query::HttpCaptureArguments = parse(arguments)?;
                json_value(self.application.capture_query(args.into_query()).await?)
            }
            "http_capture_get" => {
                let args: HttpCaptureDetailArguments = parse(arguments)?;
                json_value(
                    self.application
                        .capture_get_detail(args.session_id, args.runtime_epoch)
                        .await?,
                )
            }
            "breakpoint_query" => {
                let args: BreakpointQueryArguments = parse(arguments)?;
                json_value(self.application.breakpoint_query(args.runtime_epoch))
            }
            "breakpoint_get" => {
                let args: BreakpointDetailArguments = parse(arguments)?;
                json_value(
                    self.application
                        .breakpoint_get(args.breakpoint_id, args.runtime_epoch)?,
                )
            }
            _ => unknown_tool(name),
        }
    }

    pub(super) async fn call_configuration_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "http_rule_list" => json_value(self.application.rule_list().await?),
            "http_rule_get" => {
                let args: HttpRuleArguments = parse(arguments)?;
                json_value(self.application.rule_get(args.rule_id).await?)
            }
            "protocol_rule_list" => json_value(self.application.protocol_rule_list().await?),
            "workspace_protocol_rule_list" => {
                let args: WorkspaceArguments = parse(arguments)?;
                json_value(
                    self.application
                        .workspace_get(args.workspace_id)
                        .await?
                        .protocol_rules,
                )
            }
            "protocol_package_list" => json_value(self.application.protocol_package_list().await?),
            "protocol_package_catalog" => {
                json_value(self.application.listener_protocol_package_catalog().await?)
            }
            "protocol_package_detail" => {
                let args: ProtocolPackageArguments = parse(arguments)?;
                json_value(
                    self.application
                        .protocol_package_detail(args.package)
                        .await?,
                )
            }
            "protocol_package_usage" => {
                let args: ProtocolPackageArguments = parse(arguments)?;
                json_value(
                    self.application
                        .protocol_package_usage(args.package)
                        .await?,
                )
            }
            _ => unknown_tool(name),
        }
    }

    pub(super) async fn call_runtime_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "external_package_service_status" => {
                json_value(self.application.external_package_service_status().await?)
            }
            "android_adb_get" => json_value(self.application.android_adb_get().await?),
            "android_device_list" => json_value(self.application.android_device_list().await?),
            "android_package_list" => {
                let args: AndroidDeviceArguments = parse(arguments)?;
                json_value(self.application.android_package_list(args.serial).await?)
            }
            "android_package_get" => {
                let args: AndroidPackageArguments = parse(arguments)?;
                json_value(
                    self.application
                        .android_package_get(args.serial, args.package_name)
                        .await?,
                )
            }
            "android_profile_list" => {
                json_value(self.application.device_network_profile_list().await?)
            }
            "android_profile_get" => {
                let args: AndroidProfileArguments = parse(arguments)?;
                json_value(
                    self.application
                        .device_network_profile_get(args.profile_id)
                        .await?,
                )
            }
            "android_network_status" => {
                let args: AndroidDeviceArguments = parse(arguments)?;
                json_value(self.application.device_network_status(args.serial).await?)
            }
            "android_runtime_owner_list" => {
                json_value(self.application.device_network_runtime_owners().await?)
            }
            "android_network_endpoints" => {
                let args: AndroidEndpointArguments = parse(arguments)?;
                json_value(
                    self.application
                        .device_network_endpoints(args.serial, args.profile_id)
                        .await?,
                )
            }
            "certificate_overview" => json_value(self.application.certificate_overview().await?),
            "workspace_certificate_overview" => {
                let args: WorkspaceArguments = parse(arguments)?;
                json_value(
                    self.application
                        .listener_certificate_overview(args.workspace_id)
                        .await?,
                )
            }
            _ => unknown_tool(name),
        }
    }
}

fn parse_environment<T: serde::de::DeserializeOwned>(arguments: Value) -> Result<T, ToolFailure> {
    serde_json::from_value(arguments).map_err(|_| environment_arguments_failure())
}

fn environment_candidate_id(value: String) -> Result<EnvironmentCandidateId, ToolFailure> {
    EnvironmentCandidateId::new(value).map_err(|_| environment_arguments_failure())
}

fn environment_arguments_failure() -> ToolFailure {
    ToolFailure {
        code: "MCP_TOOL_ARGUMENTS_INVALID".to_owned(),
        message: "environment tool arguments violate the published schema".to_owned(),
        details: None,
    }
}

fn environment_lifecycle_failure(error: &EnvironmentCandidateLifecycleError) -> ToolFailure {
    let (code, message) = match error {
        EnvironmentCandidateLifecycleError::CandidateCapacityExceeded => (
            "CANDIDATE_CAPACITY_EXCEEDED",
            "environment candidate capacity was exceeded",
        ),
        EnvironmentCandidateLifecycleError::TargetCandidateAlreadyActive => (
            "TARGET_CANDIDATE_ALREADY_ACTIVE",
            "the target already has an active environment candidate",
        ),
        EnvironmentCandidateLifecycleError::ApplyAlreadyActive => (
            "APPLY_ALREADY_ACTIVE",
            "an environment apply task is already active",
        ),
        EnvironmentCandidateLifecycleError::TokenConsumed => (
            "TOKEN_CONSUMED",
            "the environment confirmation token was already consumed",
        ),
        EnvironmentCandidateLifecycleError::ConfirmationTokenMissing => (
            "CONFIRMATION_TOKEN_MISSING",
            "the environment confirmation token is missing",
        ),
        EnvironmentCandidateLifecycleError::ConfirmationTokenInvalid => (
            "CONFIRMATION_TOKEN_INVALID",
            "the environment confirmation token is invalid",
        ),
        EnvironmentCandidateLifecycleError::CandidateNotFound => (
            "CANDIDATE_NOT_FOUND",
            "the environment candidate was not found",
        ),
        EnvironmentCandidateLifecycleError::ShutdownInProgress => (
            "SHUTDOWN_IN_PROGRESS",
            "application shutdown is in progress",
        ),
        EnvironmentCandidateLifecycleError::PrivateMaterialEncodingFailed
        | EnvironmentCandidateLifecycleError::TerminalProjectionEncodingFailed
        | EnvironmentCandidateLifecycleError::InvalidState
        | EnvironmentCandidateLifecycleError::ValidatedTargetMismatch
        | EnvironmentCandidateLifecycleError::CandidateEpochExhausted
        | EnvironmentCandidateLifecycleError::InvalidPolicy => (
            "VALIDATION_LAYER_FAILED",
            "the environment operation failed before producing a public result",
        ),
    };
    ToolFailure {
        code: code.to_owned(),
        message: message.to_owned(),
        details: None,
    }
}

fn transport_projection(
    capabilities: &crate::mcp::server::McpTransportCapabilities,
) -> EnvironmentTransportProjection {
    let ip = |binding: &crate::mcp::server::McpIpCapability| EnvironmentIpBindingProjection {
        available: binding.available(),
        bind_address: binding.bind_address(),
        port: binding.port(),
        warning_codes: binding
            .warning_codes()
            .iter()
            .copied()
            .map(crate::mcp::server::McpTransportWarningCode::as_str)
            .collect(),
    };
    EnvironmentTransportProjection {
        endpoint: format!(
            "http://{}:{}/mcp",
            capabilities.ipv4().bind_address(),
            capabilities.ipv4().port()
        ),
        ipv4: ip(capabilities.ipv4()),
        ipv6: ip(capabilities.ipv6()),
        warnings: capabilities
            .warnings()
            .iter()
            .copied()
            .map(crate::mcp::server::McpTransportWarningCode::as_str)
            .collect(),
    }
}
