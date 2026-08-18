//! Read-only MCP backend implemented exclusively through the application facade.

mod dispatch;

use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppErrorViewModel, Application, BreakpointId, DiagnosticLogQuery, ProtocolPackageRef,
    RuleId, RuntimeEpoch, SessionId, SocketCaptureId, WorkspaceId,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use super::{query, resources};

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
        Self {
            code: view_model.code.clone(),
            message: view_model.message.clone(),
            details: serde_json::to_value(view_model).ok(),
        }
    }
}

pub type ToolResult = Result<Value, ToolFailure>;

#[async_trait]
pub trait ReadOnlyMcpBackend: Debug + Send + Sync {
    async fn call_tool(&self, name: &str, arguments: Value) -> ToolResult;
    async fn read_resource(&self, uri: &str) -> ToolResult;
}

#[derive(Debug)]
pub struct ApplicationBackend {
    application: Arc<Application>,
}

impl ApplicationBackend {
    pub fn new(application: Arc<Application>) -> Self {
        Self { application }
    }

    async fn application_snapshot(&self) -> ToolResult {
        const MAX_ATTEMPTS: u8 = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let first = self.read_snapshot().await?;
            let second = self.read_snapshot().await?;
            if first == second {
                return Ok(json!({
                    "snapshot": second,
                    "consistency": {
                        "strategy": "bounded_generation_validation",
                        "attempt": attempt,
                        "generation": snapshot_fingerprint(&first),
                        "observed_at": Utc::now(),
                        "status": "validated_no_observed_change"
                    }
                }));
            }
        }
        Err(ToolFailure {
            code: "SNAPSHOT_UNSTABLE".to_owned(),
            message: "Application state changed during all bounded snapshot attempts.".to_owned(),
            details: Some(json!({ "attempts": MAX_ATTEMPTS })),
        })
    }

    async fn read_snapshot(&self) -> ToolResult {
        let settings = self.application.settings_get().await?;
        let workspaces = self.application.workspace_list().await?;
        let mut workspace_details = Vec::with_capacity(workspaces.len());
        for workspace in &workspaces {
            workspace_details.push(self.application.workspace_get(workspace.id).await?);
        }
        let entry_statuses = self.application.listener_statuses().await?;
        let protocol_packages = self.application.protocol_package_list().await?;
        let diagnostics = self
            .application
            .diagnostic_log_query(&DiagnosticLogQuery::default());
        json_value(json!({
            "settings": settings,
            "workspaces": workspaces,
            "workspace_details": workspace_details,
            "entry_statuses": entry_statuses,
            "protocol_packages": protocol_packages,
            "diagnostics": diagnostics,
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
                let (category, ui_path, action, verification) =
                    if contains_any(&normalized, &["tls", "certificate", "证书", "trust anchor"])
                    {
                        (
                            "tls",
                            "入口配置 > App 接入安全 / Server 上游",
                            "核对报错方向的证书链、主机名和信任来源；如应用使用自定义信任策略，也一并核对。",
                            "在应用中重新执行对应连接测试，并确认 TCP 与 TLS 结果分别成功。",
                        )
                    } else if contains_any(
                        &normalized,
                        &["address in use", "bind", "端口", "listen"],
                    ) {
                        (
                            "bind",
                            "入口配置 > 监听地址与端口",
                            "检查同一地址和端口是否已被其他入口或进程占用，然后修改冲突配置。",
                            "重新启动该入口并确认状态为运行中。",
                        )
                    } else if contains_any(&normalized, &["dns", "resolve", "解析主机"])
                    {
                        (
                            "dns",
                            "入口配置 > Server 上游",
                            "核对 Server 主机名、当前电脑 DNS 和网络可达性。",
                            "重新执行 Server 连接测试并确认 DNS 与 TCP 分阶段成功。",
                        )
                    } else if contains_any(&normalized, &["timeout", "超时"] ) {
                        (
                            "timeout",
                            "设置 > 超时与容量",
                            "先确认超时发生在连接、写入还是读取阶段，再核对对应上游可达性和超时值。",
                            "复现请求并比较新的诊断阶段与耗时。",
                        )
                    } else if contains_any(
                        &normalized,
                        &["frame", "decode", "encode", "schema", "协议包"],
                    ) {
                        (
                            "protocol_package",
                            "协议包 > 版本详情；规则 > Socket",
                            "核对入口绑定的精确包版本、方向 Schema、字段类型和报错入口。修改包时提升 SemVer 后重新导入。",
                            "用同一测试报文复现，并确认 Frame、解析、规则、编码和写出阶段依次成功。",
                        )
                    } else {
                        (
                            "general",
                            "日志 > 诊断详情",
                            "按诊断中的对象、方向和阶段检查配置；不要把下层成功当作业务成功。",
                            "再次复现并比较错误码、阶段、时间和对象是否变化。",
                        )
                    };
                json!({
                    "diagnostic_event_id": row.event_id,
                    "category": category,
                    "evidence": row,
                    "suggested_ui_path": ui_path,
                    "suggested_action": action,
                    "risk": "尚未执行；修改配置前记录原值以便回退。",
                    "expected_verification": verification,
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
        match name {
            "application_snapshot"
            | "settings_get"
            | "workspace_list"
            | "workspace_get"
            | "entry_overview"
            | "entry_status_list"
            | "diagnostics_query"
            | "diagnose_recent_failures" => self.call_general_tool(name, arguments).await,
            "http_capture_query"
            | "http_capture_get"
            | "session_query"
            | "session_get"
            | "socket_capture_query"
            | "socket_capture_get"
            | "breakpoint_query"
            | "breakpoint_get" => self.call_traffic_tool(name, arguments).await,
            "http_rule_list"
            | "http_rule_get"
            | "protocol_rule_list"
            | "workspace_protocol_rule_list"
            | "protocol_package_list"
            | "protocol_package_catalog"
            | "protocol_package_detail"
            | "protocol_package_usage" => self.call_configuration_tool(name, arguments).await,
            "android_adb_get"
            | "android_device_list"
            | "android_package_list"
            | "android_package_get"
            | "android_profile_list"
            | "android_profile_get"
            | "android_network_status"
            | "android_runtime_owner"
            | "android_network_endpoints"
            | "certificate_overview"
            | "workspace_certificate_overview" => self.call_runtime_tool(name, arguments).await,
            _ => unknown_tool(name),
        }
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
struct HttpCaptureDetailArguments {
    session_id: SessionId,
    runtime_epoch: RuntimeEpoch,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionDetailArguments {
    session_id: SessionId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SocketCaptureDetailArguments {
    capture_id: SocketCaptureId,
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
    package_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AndroidProfileArguments {
    profile_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AndroidEndpointArguments {
    profile_id: Option<String>,
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

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn unknown_tool(name: &str) -> ToolResult {
    Err(ToolFailure::not_found(format!("Unknown tool: {name}")))
}

fn snapshot_fingerprint(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("{hash:016x}")
}
