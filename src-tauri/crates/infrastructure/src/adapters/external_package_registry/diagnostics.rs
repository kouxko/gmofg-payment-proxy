//! 外部软件包服务与连接生命周期到统一 diagnostics/MCP 的安全投影。
//!
//! 这里只记录阶段、稳定错误码、精确包身份、连接 ID 和网络端点。第三方业务报文、
//! JSON-RPC `data`、注册原文和传输错误原文都不得进入该可查询边界。

use std::net::SocketAddr;

use chrono::Utc;
use intercept_proxy_application::{
    AppError, DiagnosticLogEntryViewModel, DiagnosticLogLevel, DiagnosticLogStage, UiEventPayload,
};
use intercept_proxy_domain::{ListenerId, ProtocolPackageRef};

use super::{ExternalPackageConnectionId, ExternalPackageRegistryAdapter, recent_error_view};
use crate::adapters::PackageTransportError;

impl ExternalPackageRegistryAdapter {
    pub(super) fn publish_service_listening(&self, websocket_url: &str) {
        self.publish_diagnostic(
            DiagnosticLogLevel::Info,
            "外部软件包服务正在监听",
            format!(
                "event=service_listening; websocket_url={websocket_url}; fixed_path=/packages; authentication=disabled"
            ),
            None,
            None,
        );
    }

    pub(super) fn publish_service_failed(&self, websocket_url: &str) {
        self.publish_diagnostic(
            DiagnosticLogLevel::Error,
            "外部软件包服务监听失败",
            format!(
                "event=service_failed; websocket_url={websocket_url}; code=EXTERNAL_PACKAGE_SERVICE_BIND_FAILED"
            ),
            None,
            None,
        );
    }

    pub(super) fn publish_connection_online(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        remote_address: SocketAddr,
    ) {
        let package = package_identity(package);
        self.publish_diagnostic(
            DiagnosticLogLevel::Info,
            "外部软件包连接已上线",
            format!(
                "event=connected; package={package}; connection_id={}; remote_address={remote_address}",
                connection_id.as_uuid()
            ),
            Some(package),
            None,
        );
    }

    pub(super) fn publish_connection_offline(
        &self,
        package: &ProtocolPackageRef,
        connection_id: ExternalPackageConnectionId,
        reason: &PackageTransportError,
    ) {
        let package = package_identity(package);
        let error = recent_error_view(reason);
        let level = if matches!(reason, PackageTransportError::Disconnected) {
            DiagnosticLogLevel::Warning
        } else {
            DiagnosticLogLevel::Error
        };
        self.publish_diagnostic(
            level,
            "外部软件包连接已离线",
            format!(
                "event=disconnected; package={package}; connection_id={}; code={}; message={}",
                connection_id.as_uuid(),
                error.code,
                error.message
            ),
            Some(package),
            None,
        );
    }

    pub(crate) fn record_connection_attempt_failure(
        &self,
        phase: &'static str,
        remote_address: SocketAddr,
        code: &str,
    ) {
        self.publish_diagnostic(
            DiagnosticLogLevel::Warning,
            "外部软件包连接建立失败",
            format!(
                "event=connection_attempt_failed; phase={phase}; remote_address={remote_address}; code={code}"
            ),
            Some(remote_address.to_string()),
            None,
        );
    }

    pub(crate) fn record_registration_failure(
        &self,
        remote_address: SocketAddr,
        reason: &PackageTransportError,
    ) {
        let error = recent_error_view(reason);
        self.record_connection_attempt_failure("registration", remote_address, &error.code);
    }

    pub(crate) fn record_application_failure(
        &self,
        phase: &'static str,
        remote_address: SocketAddr,
        package: Option<&ProtocolPackageRef>,
        error: &AppError,
    ) {
        let package_text = package.map_or_else(|| "unknown".to_owned(), package_identity);
        self.publish_diagnostic(
            DiagnosticLogLevel::Error,
            "外部软件包注册处理失败",
            format!(
                "event=registration_rejected; phase={phase}; package={package_text}; remote_address={remote_address}; code={}",
                error.view_model.code
            ),
            package.map(package_identity),
            None,
        );
    }

    pub(crate) fn record_listener_stop_failure(
        &self,
        package: &ProtocolPackageRef,
        listener_id: ListenerId,
        error: &AppError,
    ) {
        let package = package_identity(package);
        self.publish_diagnostic(
            DiagnosticLogLevel::Error,
            "外部软件包离线后停止引用入口失败",
            format!(
                "event=listener_stop_failed; package={package}; listener_id={listener_id}; code={}",
                error.view_model.code
            ),
            Some(package),
            Some(listener_id.to_string()),
        );
    }

    pub(crate) fn record_listener_stopped_after_disconnect(
        &self,
        package: &ProtocolPackageRef,
        listener_id: ListenerId,
    ) {
        let package = package_identity(package);
        self.publish_diagnostic(
            DiagnosticLogLevel::Warning,
            "外部软件包离线后已停止引用入口",
            format!(
                "event=listener_stopped_after_external_package_offline; package={package}; listener_id={listener_id}"
            ),
            Some(package),
            Some(listener_id.to_string()),
        );
    }

    pub(crate) fn record_package_operation_failure(
        &self,
        phase: &'static str,
        package: &ProtocolPackageRef,
        error: &AppError,
    ) {
        let package = package_identity(package);
        self.publish_diagnostic(
            DiagnosticLogLevel::Error,
            "外部软件包生命周期处理失败",
            format!(
                "event=package_operation_failed; phase={phase}; package={package}; code={}",
                error.view_model.code
            ),
            Some(package),
            None,
        );
    }

    fn publish_diagnostic(
        &self,
        level: DiagnosticLogLevel,
        summary: &str,
        detail: String,
        entity_id: Option<String>,
        listener_id: Option<String>,
    ) {
        let Some(events) = self.events.read().clone() else {
            return;
        };
        let entry = DiagnosticLogEntryViewModel {
            level,
            stage: DiagnosticLogStage::Socket,
            summary: summary.to_owned(),
            detail: Some(detail),
            device_serial: None,
            listener_id,
            profile_id: None,
            socket_context: None,
        }
        .sanitized();
        events.publish(
            None,
            Utc::now(),
            entity_id,
            None,
            UiEventPayload::DiagnosticLogAdded(Box::new(entry)),
        );
    }
}

fn package_identity(package: &ProtocolPackageRef) -> String {
    format!("{}@{}", package.id, package.version)
}
