use std::sync::Arc;

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    DiagnosticLogEntryViewModel, DiagnosticLogLevel, DiagnosticLogStage, EventHub,
    SocketCaptureFailureDiagnostic, SocketConnectionRouteViewModel,
    SocketDiagnosticContextViewModel, SocketRelayRouteEvidenceViewModel, UiEventPayload,
};
use intercept_proxy_runtime::{
    BoundedSocketConnectionObserver, SocketConnectionEvent, SocketConnectionObserver,
    SocketConnectionTarget, SocketOpenedEvidence, SocketRelayBytes, SocketRelayDirection,
    SocketRelayFailure, SocketRelayRunContext, SocketRelayStage, SocketTransportMode,
};

const DEFAULT_SOCKET_DIAGNOSTIC_CAPACITY: usize = 256;
const SOCKET_DIAGNOSTIC_LOGICAL_BYTES: usize = 1024 * 1024;

#[path = "socket_diagnostics/mapping.rs"]
mod mapping;
use mapping::{
    application_direction, application_stage, capture_failure, diagnostic_stage, route_from_target,
    tls_evidence,
};

#[derive(Debug)]
pub(super) struct SocketDiagnosticObserver {
    retained: BoundedSocketConnectionObserver,
    events: Arc<EventHub>,
}

impl SocketDiagnosticObserver {
    pub(super) fn new(events: Arc<EventHub>) -> Self {
        Self::with_capacity(events, DEFAULT_SOCKET_DIAGNOSTIC_CAPACITY)
    }

    pub(super) fn with_capacity(events: Arc<EventHub>, capacity: usize) -> Self {
        Self {
            retained: BoundedSocketConnectionObserver::with_limits(
                capacity,
                SOCKET_DIAGNOSTIC_LOGICAL_BYTES,
            ),
            events,
        }
    }
}

impl SocketConnectionObserver for SocketDiagnosticObserver {
    fn record(&self, event: SocketConnectionEvent) {
        self.retained.record(event.clone());
        let (run, occurred_at, entity_id, entry) = diagnostic_entry(&event);
        self.events.publish(
            Some(run.workspace_runtime_epoch),
            occurred_at,
            entity_id,
            None,
            UiEventPayload::DiagnosticLogAdded(entry.sanitized()),
        );
    }

    fn begin_run(&self) {
        self.retained.begin_run();
    }

    fn retained_diagnostic_evictions(&self) -> u64 {
        self.retained.retained_diagnostic_evictions()
    }
}

fn diagnostic_entry(
    event: &SocketConnectionEvent,
) -> (
    &intercept_proxy_runtime::SocketRelayRunContext,
    DateTime<Utc>,
    Option<String>,
    DiagnosticLogEntryViewModel,
) {
    match event {
        SocketConnectionEvent::Rejected {
            run,
            peer,
            reason,
            code,
        } => (
            run,
            Utc::now(),
            None,
            rejected_entry(run, peer, *reason, code),
        ),
        SocketConnectionEvent::Admitted {
            run,
            peer,
            target,
            mode,
            at,
            connection_id,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            DiagnosticLogEntryViewModel {
                level: DiagnosticLogLevel::Info,
                stage: DiagnosticLogStage::Socket,
                summary: "Socket 连接已接纳".into(),
                detail: Some(admitted_detail(run, *connection_id, peer, target, mode)),
                device_serial: None,
                listener_id: Some(run.listener_id.clone()),
                profile_id: None,
                socket_context: Some(socket_context(
                    run,
                    Some(*connection_id),
                    Some(route_from_target(target)),
                    None,
                    SocketRelayStage::Admission,
                    None,
                    SocketRelayBytes::default(),
                )),
            },
        ),
        SocketConnectionEvent::Opened {
            run,
            evidence,
            at,
            connection_id,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            opened_entry(run, *connection_id, evidence),
        ),
        SocketConnectionEvent::RequestParsed {
            run,
            connection_id,
            preview,
            at,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            request_parsed_entry(run, *connection_id, preview),
        ),
        SocketConnectionEvent::Closed {
            run,
            target,
            opened,
            bytes,
            failure,
            at,
            connection_id,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            closed_entry(
                run,
                *connection_id,
                target,
                *opened,
                *bytes,
                failure.as_ref(),
            ),
        ),
    }
}

fn request_parsed_entry(
    run: &SocketRelayRunContext,
    connection_id: uuid::Uuid,
    preview: &intercept_proxy_runtime::SocketLocalRequestPreview,
) -> DiagnosticLogEntryViewModel {
    let document_shape = preview.document.as_ref().map_or_else(
        || "Hex（request Decode 关闭）".to_owned(),
        |document| {
            format!(
                "Schema {}@{}，{} 个预览字段，截断：{}",
                document.schema_id,
                document.schema_version,
                document.fields.len(),
                document.truncated
            )
        },
    );
    DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Info,
        stage: DiagnosticLogStage::Socket,
        summary: "Socket 本地请求已解析".into(),
        // 通用诊断日志只保留有界形状元数据；字段值仍只存在 Proxy 的有界 RequestParsed
        // observer 事件中，避免复制到第二个无关队列。
        detail: Some(format!(
            "listener-run：{}；连接：{connection_id}；交换：{}；request：{} 字节；原始预览：{} 字节；{document_shape}",
            run.listener_run_epoch,
            preview.exchange_id,
            preview.origin_len,
            preview.origin_preview.len()
        )),
        device_serial: None,
        listener_id: Some(run.listener_id.clone()),
        profile_id: None,
        socket_context: Some(socket_context(
            run,
            Some(connection_id),
            Some(SocketConnectionRouteViewModel::LocalResponder {
                downstream_tls_peer: None,
            }),
            None,
            SocketRelayStage::FrameProcess,
            Some(SocketRelayDirection::LocalExchange),
            SocketRelayBytes::default(),
        )),
    }
}

fn admitted_detail(
    run: &SocketRelayRunContext,
    connection_id: uuid::Uuid,
    peer: &std::net::SocketAddr,
    target: &SocketConnectionTarget,
    mode: &SocketTransportMode,
) -> String {
    match target {
        SocketConnectionTarget::Relay(target) => format!(
            "listener-run：{}；连接：{connection_id}；客户端：{peer}；目标：{target}；模式：{mode:?}",
            run.listener_run_epoch
        ),
        SocketConnectionTarget::LocalResponder => format!(
            "listener-run：{}；连接：{connection_id}；客户端：{peer}；处理：本地应答（无上游）；App 侧传输：{}",
            run.listener_run_epoch,
            downstream_transport_text(mode)
        ),
    }
}

fn opened_entry(
    run: &SocketRelayRunContext,
    connection_id: uuid::Uuid,
    evidence: &SocketOpenedEvidence,
) -> DiagnosticLogEntryViewModel {
    let (summary, detail, stage, route) = match evidence {
        SocketOpenedEvidence::Relay {
            resolved_address,
            downstream_tls_peer,
            upstream_tls,
        } => (
            "Socket 上游连接已建立",
            format!(
                "listener-run：{}；连接：{connection_id}；上游：{resolved_address}；下游 TLS：{}；上游 TLS：{}",
                run.listener_run_epoch,
                downstream_tls_peer.as_deref().unwrap_or("未启用"),
                upstream_tls
                    .as_ref()
                    .map_or("未启用", |tls| tls.tls_version.as_str())
            ),
            SocketRelayStage::Connect,
            SocketConnectionRouteViewModel::Relay(Box::new(SocketRelayRouteEvidenceViewModel {
                configured_address: None,
                resolved_address: Some(resolved_address.to_string()),
                downstream_tls_peer: downstream_tls_peer.clone(),
                upstream_tls: upstream_tls.as_ref().map(tls_evidence),
                connection_test: None,
            })),
        ),
        SocketOpenedEvidence::LocalResponder {
            downstream_tls_peer,
        } => {
            let stage = if downstream_tls_peer.is_some() {
                SocketRelayStage::DownstreamTls
            } else {
                SocketRelayStage::Admission
            };
            (
                "Socket 本地应答已就绪",
                format!(
                    "listener-run：{}；连接：{connection_id}；处理：本地应答（无上游）；App 侧 TLS：{}",
                    run.listener_run_epoch,
                    downstream_tls_peer.as_deref().unwrap_or("未启用")
                ),
                stage,
                SocketConnectionRouteViewModel::LocalResponder {
                    downstream_tls_peer: downstream_tls_peer.clone(),
                },
            )
        }
    };
    DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Info,
        stage: DiagnosticLogStage::Socket,
        summary: summary.into(),
        detail: Some(detail),
        device_serial: None,
        listener_id: Some(run.listener_id.clone()),
        profile_id: None,
        socket_context: Some(socket_context(
            run,
            Some(connection_id),
            Some(route),
            None,
            stage,
            None,
            SocketRelayBytes::default(),
        )),
    }
}

fn downstream_transport_text(mode: &SocketTransportMode) -> &'static str {
    match mode {
        SocketTransportMode::Transparent => "TCP",
        SocketTransportMode::TlsToTcp => "TLS",
        SocketTransportMode::TcpToTls | SocketTransportMode::TlsToTls => "未知",
    }
}

fn rejected_entry(
    run: &SocketRelayRunContext,
    peer: &std::net::SocketAddr,
    reason: intercept_proxy_runtime::SocketRejectionReason,
    code: &str,
) -> DiagnosticLogEntryViewModel {
    DiagnosticLogEntryViewModel {
        level: DiagnosticLogLevel::Warning,
        stage: DiagnosticLogStage::Socket,
        summary: format!("Socket 连接已拒绝：{reason:?}"),
        detail: Some(format!(
            "listener-run：{}；错误码：{code}；客户端：{peer}",
            run.listener_run_epoch
        )),
        device_serial: None,
        listener_id: Some(run.listener_id.clone()),
        profile_id: None,
        socket_context: Some(socket_context(
            run,
            None,
            None,
            None,
            SocketRelayStage::Admission,
            None,
            SocketRelayBytes::default(),
        )),
    }
}

fn closed_entry(
    run: &SocketRelayRunContext,
    connection_id: uuid::Uuid,
    target: &SocketConnectionTarget,
    opened: bool,
    bytes: SocketRelayBytes,
    failure: Option<&SocketRelayFailure>,
) -> DiagnosticLogEntryViewModel {
    DiagnosticLogEntryViewModel {
        level: if failure.is_some() {
            DiagnosticLogLevel::Error
        } else {
            DiagnosticLogLevel::Info
        },
        stage: failure.map_or(DiagnosticLogStage::Socket, |failure| {
            diagnostic_stage(*failure)
        }),
        summary: if failure.is_some() {
            "Socket 连接已失败".into()
        } else {
            "Socket 连接已关闭".into()
        },
        detail: Some(format!(
            "listener-run：{}；连接：{connection_id}；{}",
            run.listener_run_epoch,
            closed_detail(target, opened, bytes, failure)
        )),
        device_serial: None,
        listener_id: Some(run.listener_id.clone()),
        profile_id: None,
        socket_context: Some(socket_context(
            run,
            Some(connection_id),
            Some(route_from_target(target)),
            failure.and_then(capture_failure),
            failure.map_or(SocketRelayStage::Shutdown, |failure| failure.stage),
            failure.and_then(|failure| failure.direction),
            bytes,
        )),
    }
}

fn socket_context(
    run: &SocketRelayRunContext,
    connection_id: Option<uuid::Uuid>,
    route: Option<SocketConnectionRouteViewModel>,
    capture_failure: Option<SocketCaptureFailureDiagnostic>,
    stage: SocketRelayStage,
    direction: Option<SocketRelayDirection>,
    bytes: SocketRelayBytes,
) -> SocketDiagnosticContextViewModel {
    SocketDiagnosticContextViewModel {
        connection_id: connection_id.map(|id| id.to_string()),
        workspace_runtime_epoch: run.workspace_runtime_epoch.to_string(),
        listener_run_epoch: run.listener_run_epoch.to_string(),
        route,
        capture_failure,
        stage: application_stage(stage),
        direction: direction.map(application_direction),
        client_to_server_read_bytes: bytes.client_to_server_read,
        client_to_server_bytes: bytes.client_to_server,
        server_to_client_read_bytes: bytes.server_to_client_read,
        server_to_client_bytes: bytes.server_to_client,
    }
}

fn closed_detail(
    target: &SocketConnectionTarget,
    opened: bool,
    bytes: intercept_proxy_runtime::SocketRelayBytes,
    failure: Option<&SocketRelayFailure>,
) -> String {
    let outcome = failure.map_or_else(
        || "成功".to_owned(),
        |failure| {
            format!(
                "错误码：{}；阶段：{:?}；方向：{}",
                failure.code,
                failure.stage,
                failure.direction.map_or("无".into(), direction_text)
            )
        },
    );
    match target {
        SocketConnectionTarget::Relay(_) => format!(
            "已建立：{opened}；客户端读取：{} 字节；客户端→上游：{} 字节；上游读取：{} 字节；上游→客户端：{} 字节；{outcome}",
            bytes.client_to_server_read,
            bytes.client_to_server,
            bytes.server_to_client_read,
            bytes.server_to_client
        ),
        SocketConnectionTarget::LocalResponder => format!(
            "已建立：{opened}；处理：本地应答（无上游）；App 请求读取：{} 字节；本地回应写出：{} 字节；{outcome}",
            bytes.client_to_server_read, bytes.server_to_client
        ),
    }
}

fn direction_text(direction: SocketRelayDirection) -> String {
    match direction {
        SocketRelayDirection::Downstream => "下游 TLS".into(),
        SocketRelayDirection::Upstream => "上游 TLS".into(),
        SocketRelayDirection::ClientToServer => "客户端→上游".into(),
        SocketRelayDirection::ServerToClient => "上游→客户端".into(),
        SocketRelayDirection::LocalExchange => "本地请求→应答".into(),
    }
}

#[cfg(test)]
#[path = "socket_diagnostics_tests.rs"]
mod tests;
