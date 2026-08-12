use std::sync::Arc;

use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    DiagnosticLogEntryViewModel, DiagnosticLogLevel, DiagnosticLogStage, EventHub,
    SocketDiagnosticContextViewModel, SocketDiagnosticDirection, SocketDiagnosticStage,
    UiEventPayload,
};
use intercept_proxy_runtime::{
    BoundedSocketConnectionObserver, SocketConnectionEvent, SocketConnectionObserver,
    SocketRelayBytes, SocketRelayDirection, SocketRelayFailure, SocketRelayRunContext,
    SocketRelayStage,
};

const DEFAULT_SOCKET_DIAGNOSTIC_CAPACITY: usize = 4_096;

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
            retained: BoundedSocketConnectionObserver::new(capacity),
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
                detail: Some(format!(
                    "listener-run：{}；连接：{connection_id}；客户端：{peer}；目标：{target}；模式：{mode:?}",
                    run.listener_run_epoch
                )),
                device_serial: None,
                listener_id: Some(run.listener_id.clone()),
                profile_id: None,
                socket_context: Some(socket_context(
                    run,
                    Some(*connection_id),
                    SocketRelayStage::Admission,
                    None,
                    SocketRelayBytes::default(),
                )),
            },
        ),
        SocketConnectionEvent::Opened {
            run,
            resolved_address,
            downstream_tls_peer,
            upstream_tls,
            at,
            connection_id,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            DiagnosticLogEntryViewModel {
                level: DiagnosticLogLevel::Info,
                stage: DiagnosticLogStage::Socket,
                summary: "Socket 上游连接已建立".into(),
                detail: Some(format!(
                    "listener-run：{}；连接：{connection_id}；上游：{resolved_address}；下游 TLS：{}；上游 TLS：{}",
                    run.listener_run_epoch,
                    downstream_tls_peer.as_deref().unwrap_or("未启用"),
                    upstream_tls
                        .as_ref()
                        .map_or("未启用", |tls| tls.tls_version.as_str())
                )),
                device_serial: None,
                listener_id: Some(run.listener_id.clone()),
                profile_id: None,
                socket_context: Some(socket_context(
                    run,
                    Some(*connection_id),
                    SocketRelayStage::Connect,
                    None,
                    SocketRelayBytes::default(),
                )),
            },
        ),
        SocketConnectionEvent::Closed {
            run,
            opened,
            bytes,
            failure,
            at,
            connection_id,
        } => (
            run,
            (*at).into(),
            Some(connection_id.to_string()),
            closed_entry(run, *connection_id, *opened, *bytes, failure.as_ref()),
        ),
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
            SocketRelayStage::Admission,
            None,
            SocketRelayBytes::default(),
        )),
    }
}

fn closed_entry(
    run: &SocketRelayRunContext,
    connection_id: uuid::Uuid,
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
            closed_detail(opened, bytes, failure)
        )),
        device_serial: None,
        listener_id: Some(run.listener_id.clone()),
        profile_id: None,
        socket_context: Some(socket_context(
            run,
            Some(connection_id),
            failure.map_or(SocketRelayStage::Shutdown, |failure| failure.stage),
            failure.and_then(|failure| failure.direction),
            bytes,
        )),
    }
}

fn socket_context(
    run: &SocketRelayRunContext,
    connection_id: Option<uuid::Uuid>,
    stage: SocketRelayStage,
    direction: Option<SocketRelayDirection>,
    bytes: SocketRelayBytes,
) -> SocketDiagnosticContextViewModel {
    SocketDiagnosticContextViewModel {
        connection_id: connection_id.map(|id| id.to_string()),
        workspace_runtime_epoch: run.workspace_runtime_epoch.to_string(),
        listener_run_epoch: run.listener_run_epoch.to_string(),
        stage: application_stage(stage),
        direction: direction.map(application_direction),
        client_to_server_bytes: bytes.client_to_server,
        server_to_client_bytes: bytes.server_to_client,
    }
}

const fn application_stage(stage: SocketRelayStage) -> SocketDiagnosticStage {
    match stage {
        SocketRelayStage::Admission => SocketDiagnosticStage::Admission,
        SocketRelayStage::DownstreamTls => SocketDiagnosticStage::DownstreamTls,
        SocketRelayStage::Dns => SocketDiagnosticStage::Dns,
        SocketRelayStage::Connect => SocketDiagnosticStage::Connect,
        SocketRelayStage::UpstreamTls => SocketDiagnosticStage::UpstreamTls,
        SocketRelayStage::RelayRead => SocketDiagnosticStage::RelayRead,
        SocketRelayStage::RelayWrite => SocketDiagnosticStage::RelayWrite,
        SocketRelayStage::Shutdown => SocketDiagnosticStage::Shutdown,
    }
}

const fn application_direction(direction: SocketRelayDirection) -> SocketDiagnosticDirection {
    match direction {
        SocketRelayDirection::Downstream => SocketDiagnosticDirection::Downstream,
        SocketRelayDirection::Upstream => SocketDiagnosticDirection::Upstream,
        SocketRelayDirection::ClientToServer => SocketDiagnosticDirection::ClientToServer,
        SocketRelayDirection::ServerToClient => SocketDiagnosticDirection::ServerToClient,
    }
}

fn diagnostic_stage(failure: SocketRelayFailure) -> DiagnosticLogStage {
    match failure.stage {
        SocketRelayStage::DownstreamTls => DiagnosticLogStage::DownstreamTls,
        SocketRelayStage::UpstreamTls => DiagnosticLogStage::UpstreamTls,
        SocketRelayStage::Admission
        | SocketRelayStage::Dns
        | SocketRelayStage::Connect
        | SocketRelayStage::RelayRead
        | SocketRelayStage::RelayWrite
        | SocketRelayStage::Shutdown => DiagnosticLogStage::Socket,
    }
}

fn closed_detail(
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
    format!(
        "已建立：{opened}；客户端→上游：{} 字节；上游→客户端：{} 字节；{outcome}",
        bytes.client_to_server, bytes.server_to_client
    )
}

fn direction_text(direction: SocketRelayDirection) -> String {
    match direction {
        SocketRelayDirection::Downstream => "下游 TLS".into(),
        SocketRelayDirection::Upstream => "上游 TLS".into(),
        SocketRelayDirection::ClientToServer => "客户端→上游".into(),
        SocketRelayDirection::ServerToClient => "上游→客户端".into(),
    }
}

#[cfg(test)]
#[path = "socket_diagnostics_tests.rs"]
mod tests;
