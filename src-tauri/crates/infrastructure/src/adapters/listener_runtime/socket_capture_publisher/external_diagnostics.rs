//! 外部协议包 RPC 失败到统一 diagnostics/MCP 事件的旁路投影。

use chrono::Utc;
use intercept_proxy_application::{
    DiagnosticLogEntryViewModel, DiagnosticLogLevel, DiagnosticLogStage,
    ExternalPackageCallDiagnosticViewModel, ExternalPackageCallStage,
    SocketDiagnosticContextViewModel, SocketDiagnosticDirection, SocketDiagnosticStage,
    UiEventPayload,
};
use intercept_proxy_domain::ProtocolDirection;
use intercept_proxy_runtime::SocketConnectionIdentity;

use super::SocketCaptureContext;

impl SocketCaptureContext {
    /// 发布一次外部 JSON-RPC 失败的脱敏、可查询诊断。
    pub(in crate::adapters::listener_runtime) fn record_external_rpc_failure(
        &self,
        connection: &SocketConnectionIdentity,
        call: ExternalPackageCallDiagnosticViewModel,
    ) {
        let Some(publisher) = &self.publisher else {
            return;
        };
        let call = call.sanitized();
        let diagnostic_stage = match call.stage {
            ExternalPackageCallStage::Frame => SocketDiagnosticStage::FrameInspect,
            ExternalPackageCallStage::Decode => SocketDiagnosticStage::Decode,
            ExternalPackageCallStage::Encode => SocketDiagnosticStage::Encode,
            ExternalPackageCallStage::Display => SocketDiagnosticStage::FrameProcess,
        };
        let direction = match call.direction {
            ProtocolDirection::Upstream => SocketDiagnosticDirection::ClientToServer,
            ProtocolDirection::Downstream => SocketDiagnosticDirection::ServerToClient,
        };
        let detail = format!(
            "package={}@{}; method={}; request_id={}; remote_code={}; remote_message={}; remote_data={}",
            call.package.id,
            call.package.version,
            call.method,
            call.request_id.as_deref().unwrap_or("none"),
            call.remote_code
                .map_or_else(|| "none".to_owned(), |code| code.to_string()),
            call.remote_message.as_deref().unwrap_or("none"),
            call.remote_data_summary.as_deref().unwrap_or("none"),
        );
        let entry = DiagnosticLogEntryViewModel {
            level: DiagnosticLogLevel::Error,
            stage: DiagnosticLogStage::Socket,
            summary: "外部协议包 JSON-RPC 调用失败".to_owned(),
            detail: Some(detail),
            device_serial: None,
            listener_id: Some(self.listener_id.to_string()),
            profile_id: None,
            socket_context: Some(SocketDiagnosticContextViewModel {
                connection_id: Some(connection.connection_id.to_string()),
                workspace_runtime_epoch: connection.runtime_epoch.to_string(),
                listener_run_epoch: connection.runtime_epoch.to_string(),
                route: None,
                capture_failure: None,
                external_package_call: Some(call),
                stage: diagnostic_stage,
                direction: Some(direction),
                client_to_server_read_bytes: 0,
                client_to_server_bytes: 0,
                server_to_client_read_bytes: 0,
                server_to_client_bytes: 0,
            }),
        }
        .sanitized();
        publisher.inner.events.read().publish(
            Some(connection.runtime_epoch),
            Utc::now(),
            Some(connection.connection_id.to_string()),
            None,
            UiEventPayload::DiagnosticLogAdded(entry),
        );
    }
}
