//! Tool dispatch groups kept separate from transport and protocol handling.

use serde_json::Value;

use super::{
    AndroidEndpointArguments, AndroidPackageArguments, AndroidProfileArguments, ApplicationBackend,
    BreakpointDetailArguments, BreakpointQueryArguments, HttpCaptureDetailArguments,
    HttpRuleArguments, ProtocolPackageArguments, SessionDetailArguments,
    SocketCaptureDetailArguments, ToolResult, WorkspaceArguments, json_value, parse, query,
    unknown_tool,
};

impl ApplicationBackend {
    pub(super) async fn call_general_tool(&self, name: &str, arguments: Value) -> ToolResult {
        match name {
            "application_snapshot" => self.application_snapshot().await,
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
            "session_query" => {
                let args: query::SessionArguments = parse(arguments)?;
                json_value(self.application.session_query(args.into_query()).await?)
            }
            "session_get" => {
                let args: SessionDetailArguments = parse(arguments)?;
                json_value(self.application.session_get(args.session_id).await?)
            }
            "socket_capture_query" => {
                let args: query::SocketCaptureArguments = parse(arguments)?;
                json_value(
                    self.application
                        .socket_capture_query(args.into_query())
                        .await?,
                )
            }
            "socket_capture_get" => {
                let args: SocketCaptureDetailArguments = parse(arguments)?;
                json_value(
                    self.application
                        .socket_capture_get_detail(args.capture_id)
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
            "android_adb_get" => json_value(self.application.android_adb_get().await?),
            "android_device_list" => json_value(self.application.android_device_list().await?),
            "android_package_list" => json_value(self.application.android_package_list().await?),
            "android_package_get" => {
                let args: AndroidPackageArguments = parse(arguments)?;
                json_value(
                    self.application
                        .android_package_get(args.package_name)
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
            "android_network_status" => json_value(self.application.device_network_status().await?),
            "android_runtime_owner" => {
                json_value(self.application.device_network_runtime_owner().await?)
            }
            "android_network_endpoints" => {
                let args: AndroidEndpointArguments = parse(arguments)?;
                json_value(
                    self.application
                        .device_network_endpoints(args.profile_id)
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
