use crate::{
    AndroidNetworkState, DiagnosticLogEntryViewModel, DiagnosticLogLevel,
    DiagnosticLogRowViewModel, DiagnosticLogStage, EventHub, ListenerRuntimeState, UiEventPayload,
    UiTone,
};

impl EventHub {
    /// 从有界事件回放生成统一诊断日志，不复制报文正文或敏感材料。
    #[must_use]
    pub fn diagnostic_log_snapshot(&self) -> Vec<DiagnosticLogRowViewModel> {
        self.state
            .lock()
            .retained
            .iter()
            .filter_map(|event| {
                diagnostic_entry(&event.payload).map(|entry| {
                    DiagnosticLogRowViewModel::from_entry(event.event_id, event.occurred_at, &entry)
                })
            })
            .collect()
    }
}

fn diagnostic_entry(payload: &UiEventPayload) -> Option<DiagnosticLogEntryViewModel> {
    match payload {
        UiEventPayload::DiagnosticLogAdded(entry) => Some(entry.clone()),
        UiEventPayload::ListenerStatusChanged(status) => Some(DiagnosticLogEntryViewModel {
            level: if status.state == ListenerRuntimeState::Faulted {
                DiagnosticLogLevel::Error
            } else {
                DiagnosticLogLevel::Info
            },
            stage: DiagnosticLogStage::Listener,
            summary: format!("代理入口{}", status.state_text),
            detail: status
                .fault_reason
                .clone()
                .or_else(|| Some(status.listen_address.clone())),
            device_serial: None,
            listener_id: Some(status.listener_id.to_string()),
            profile_id: None,
            socket_context: None,
        }),
        UiEventPayload::AndroidVpnStatusChanged(status) => Some(DiagnosticLogEntryViewModel {
            level: match status.state {
                AndroidNetworkState::Faulted => DiagnosticLogLevel::Error,
                AndroidNetworkState::Unknown => DiagnosticLogLevel::Warning,
                _ => DiagnosticLogLevel::Info,
            },
            stage: DiagnosticLogStage::Vpn,
            summary: format!("设备网络接管{}", status.state_text),
            detail: (!status.message.is_empty()).then(|| status.message.clone()),
            device_serial: Some(status.serial.clone()),
            listener_id: None,
            profile_id: status.active_profile_id.clone(),
            socket_context: None,
        }),
        UiEventPayload::SessionUpdated(session) => Some(DiagnosticLogEntryViewModel {
            level: level_for_tone(session.ui_tone),
            stage: DiagnosticLogStage::Http,
            summary: format!("HTTP 会话更新：{}", session.result),
            detail: Some(format!(
                "{} {}；状态：{}；耗时：{}",
                session.method,
                session.target,
                session
                    .http_status
                    .map_or_else(|| "未返回".to_owned(), |status| status.to_string()),
                session
                    .duration_ms
                    .map_or_else(|| "未完成".to_owned(), |duration| format!("{duration} ms")),
            )),
            device_serial: None,
            listener_id: Some(session.channel.as_str().to_owned()),
            profile_id: None,
            socket_context: None,
        }),
        UiEventPayload::ResourceWarning { message } => Some(DiagnosticLogEntryViewModel {
            level: DiagnosticLogLevel::Warning,
            stage: DiagnosticLogStage::System,
            summary: message.clone(),
            detail: None,
            device_serial: None,
            listener_id: None,
            profile_id: None,
            socket_context: None,
        }),
        UiEventPayload::OperationFailed(error) => {
            let stage = stage_for_error_code(&error.code);
            let (device_serial, listener_id, profile_id) =
                error_context(stage, error.entity_id.clone());
            Some(DiagnosticLogEntryViewModel {
                level: DiagnosticLogLevel::Error,
                stage,
                summary: error.message.clone(),
                detail: Some(match &error.suggested_action {
                    Some(action) => format!("错误码：{}；建议：{action}", error.code),
                    None => format!("错误码：{}", error.code),
                }),
                device_serial,
                listener_id,
                profile_id,
                socket_context: None,
            })
        }
        _ => None,
    }
}

fn level_for_tone(tone: UiTone) -> DiagnosticLogLevel {
    match tone {
        UiTone::Danger => DiagnosticLogLevel::Error,
        UiTone::Warning => DiagnosticLogLevel::Warning,
        UiTone::Neutral | UiTone::Info | UiTone::Positive => DiagnosticLogLevel::Info,
    }
}

/// 稳定错误码是诊断阶段的唯一分类依据；前端和日志页不解析中文错误文本。
pub(crate) fn stage_for_error_code(code: &str) -> DiagnosticLogStage {
    match code {
        "ANDROID_ADB_FORWARD_INVALID"
        | "ANDROID_ADB_FORWARD_CLEANUP_FAILED"
        | "ANDROID_CONTROL_SOCKET_FAILED"
        | "ANDROID_CONTROL_SOCKET_TIMEOUT"
        | "ANDROID_CONTROL_SOCKET_UNAVAILABLE" => DiagnosticLogStage::AdbForwardControl,
        "ANDROID_ADB_REVERSE_FAILED"
        | "ANDROID_ADB_REVERSE_CREATE_FAILED"
        | "ANDROID_ADB_REVERSE_CLEANUP_FAILED" => DiagnosticLogStage::AdbReverseBusiness,
        "ANDROID_ADB_COMMAND_FAILED"
        | "ANDROID_ADB_EXEC_FAILED"
        | "ANDROID_ADB_NOT_FOUND"
        | "ANDROID_ADB_SELECTED_TRANSPORT_STALE"
        | "ANDROID_ADB_TIMEOUT"
        | "ANDROID_PROTOCOL_ENCODE_FAILED"
        | "ANDROID_PROTOCOL_FRAME_INVALID"
        | "ANDROID_PROTOCOL_FRAME_TOO_LARGE"
        | "ANDROID_PROTOCOL_JSON_INVALID"
        | "ANDROID_PROTOCOL_OPERATION_INVALID"
        | "ANDROID_PROTOCOL_RESPONSE_INVALID"
        | "ANDROID_PROTOCOL_RESPONSE_MISMATCH" => DiagnosticLogStage::Companion,
        "ANDROID_VPN" | "ANDROID_NETWORK_START_FAILED" => DiagnosticLogStage::Vpn,
        "ANDROID_TUN" => DiagnosticLogStage::Tun,
        "ANDROID_PACKAGE_NAME_INVALID"
        | "ANDROID_PACKAGE_NOT_FOUND"
        | "ANDROID_PACKAGE_QUERY_TOO_LONG"
        | "APP_SELECTION" => DiagnosticLogStage::AppSelection,
        "ANDROID_ROUTE" | "ANDROID_PROXY_DESTINATION_RESOLVE_FAILED" => {
            DiagnosticLogStage::RouteActivation
        }
        "ANDROID_PROXY_LISTENER_BIND_UNREACHABLE" => DiagnosticLogStage::DesktopDns,
        "DOWNSTREAM_TLS_HANDSHAKE_FAILED"
        | "CLIENT_TLS"
        | "SOCKET_DOWNSTREAM_TLS_FAILED"
        | "SOCKET_DOWNSTREAM_TLS_TIMEOUT" => DiagnosticLogStage::DownstreamTls,
        "TLS_HANDSHAKE_FAILED"
        | "UPSTREAM_CONNECT_TIMEOUT"
        | "UPSTREAM_WRITE_TIMEOUT"
        | "UPSTREAM_READ_TIMEOUT"
        | "UPSTREAM_SECURITY_SESSION_MISSING"
        | "UPSTREAM_TLS_NOT_ENABLED"
        | "SOCKET_UPSTREAM_TLS_FAILED"
        | "SOCKET_UPSTREAM_TLS_TIMEOUT" => DiagnosticLogStage::UpstreamTls,
        "HTTP_STATUS_INVALID"
        | "SESSION_NOT_FOUND"
        | "BODY_CHARSET_MISSING"
        | "BODY_CHARSET_UNSUPPORTED"
        | "BODY_DECODE_FAILED"
        | "BODY_ENCODE_FAILED"
        | "BODY_TOO_LARGE"
        | "HEADER_INVALID"
        | "HEADER_LIMIT_EXCEEDED"
        | "JSON_INVALID"
        | "JSON_MEDIA_TYPE_REQUIRED"
        | "INCORRECT_CONTENT_LENGTH"
        | "TRUNCATED_RESPONSE"
        | "RAW_BODY_HAS_NO_TEXT"
        | "SHIFT_JIS_DECODE_FAILED"
        | "SHIFT_JIS_ENCODE_FAILED"
        | "UTF8_DECODE_FAILED" => DiagnosticLogStage::Http,
        "SOCKET_TARGET_INVALID"
        | "SOCKET_CIDR_DENIED"
        | "SOCKET_CAPACITY_EXHAUSTED"
        | "SOCKET_DNS_FAILED"
        | "SOCKET_DNS_TIMEOUT"
        | "SOCKET_CONNECT_TIMEOUT"
        | "SOCKET_CONNECT_FAILED"
        | "SOCKET_READ_TIMEOUT"
        | "SOCKET_READ_FAILED"
        | "SOCKET_WRITE_TIMEOUT"
        | "SOCKET_WRITE_FAILED"
        | "SOCKET_RELAY_CANCELLED"
        | "SOCKET_CONNECTION_TASK_PANICKED" => DiagnosticLogStage::Socket,
        "LISTENER_ALREADY_RUNNING"
        | "LISTENER_CERTIFICATE_MATERIAL_UNAVAILABLE"
        | "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
        | "LISTENER_CONNECTION_TEST_UNSUPPORTED"
        | "LISTENER_NOT_FOUND"
        | "LISTENER_NOT_RUNNING"
        | "LISTENER_REQUIRED"
        | "LISTENER_RUNTIME_ACTIVE"
        | "LISTENER_RUNTIME_NOT_READY"
        | "LISTENER_START_FAILED"
        | "LISTENER_STOP_FAILED"
        | "PROXY_ALREADY_RUNNING"
        | "PROXY_NOT_RUNNING"
        | "PROXY_START_CLEANUP_FAILED"
        | "PROXY_STOPPED"
        | "PORT_IN_USE"
        | "IO_ERROR" => DiagnosticLogStage::Listener,
        "STOP_FAILED" | "FAULT_EXECUTION_CANCELLED" => DiagnosticLogStage::StopFallback,
        "CLEANUP_FAILED" | "CERTIFICATE_CLEANUP_FAILED" => DiagnosticLogStage::Cleanup,
        _ => DiagnosticLogStage::System,
    }
}

fn error_context(
    stage: DiagnosticLogStage,
    entity_id: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    match stage {
        DiagnosticLogStage::AdbForwardControl
        | DiagnosticLogStage::AdbReverseBusiness
        | DiagnosticLogStage::Companion => (entity_id, None, None),
        DiagnosticLogStage::Vpn
        | DiagnosticLogStage::Tun
        | DiagnosticLogStage::AppSelection
        | DiagnosticLogStage::RouteActivation
        | DiagnosticLogStage::StopFallback
        | DiagnosticLogStage::Cleanup => (None, None, entity_id),
        DiagnosticLogStage::Listener
        | DiagnosticLogStage::DownstreamTls
        | DiagnosticLogStage::UpstreamTls
        | DiagnosticLogStage::Http
        | DiagnosticLogStage::Socket => (None, entity_id, None),
        DiagnosticLogStage::System | DiagnosticLogStage::DesktopDns => (None, None, None),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::{AppErrorViewModel, ChannelId, SessionSummaryViewModel};

    fn operation_failed(code: &str) -> UiEventPayload {
        UiEventPayload::OperationFailed(AppErrorViewModel {
            code: code.to_owned(),
            message: "测试失败".into(),
            field_errors: BTreeMap::new(),
            retryable: false,
            suggested_action: None,
            entity_id: Some("entity-1".into()),
            runtime_epoch: None,
        })
    }

    #[test]
    fn stable_error_codes_map_to_transport_stages() {
        for (code, expected) in [
            (
                "ANDROID_ADB_FORWARD_INVALID",
                DiagnosticLogStage::AdbForwardControl,
            ),
            (
                "ANDROID_ADB_REVERSE_FAILED",
                DiagnosticLogStage::AdbReverseBusiness,
            ),
            (
                "DOWNSTREAM_TLS_HANDSHAKE_FAILED",
                DiagnosticLogStage::DownstreamTls,
            ),
            ("TLS_HANDSHAKE_FAILED", DiagnosticLogStage::UpstreamTls),
            ("UPSTREAM_READ_TIMEOUT", DiagnosticLogStage::UpstreamTls),
            ("HTTP_STATUS_INVALID", DiagnosticLogStage::Http),
            ("SOCKET_CONNECT_TIMEOUT", DiagnosticLogStage::Socket),
            ("SOCKET_WRITE_FAILED", DiagnosticLogStage::Socket),
            (
                "SOCKET_DOWNSTREAM_TLS_TIMEOUT",
                DiagnosticLogStage::DownstreamTls,
            ),
            (
                "SOCKET_UPSTREAM_TLS_FAILED",
                DiagnosticLogStage::UpstreamTls,
            ),
        ] {
            let entry = diagnostic_entry(&operation_failed(code)).expect("diagnostic event");
            assert_eq!(entry.stage, expected, "unexpected stage for {code}");
        }
    }

    #[test]
    fn every_socket_error_code_has_an_explicit_diagnostic_stage() {
        for (code, expected) in [
            ("SOCKET_TARGET_INVALID", DiagnosticLogStage::Socket),
            ("SOCKET_CIDR_DENIED", DiagnosticLogStage::Socket),
            ("SOCKET_CAPACITY_EXHAUSTED", DiagnosticLogStage::Socket),
            ("SOCKET_DNS_FAILED", DiagnosticLogStage::Socket),
            ("SOCKET_DNS_TIMEOUT", DiagnosticLogStage::Socket),
            ("SOCKET_CONNECT_TIMEOUT", DiagnosticLogStage::Socket),
            ("SOCKET_CONNECT_FAILED", DiagnosticLogStage::Socket),
            (
                "SOCKET_DOWNSTREAM_TLS_TIMEOUT",
                DiagnosticLogStage::DownstreamTls,
            ),
            (
                "SOCKET_DOWNSTREAM_TLS_FAILED",
                DiagnosticLogStage::DownstreamTls,
            ),
            (
                "SOCKET_UPSTREAM_TLS_TIMEOUT",
                DiagnosticLogStage::UpstreamTls,
            ),
            (
                "SOCKET_UPSTREAM_TLS_FAILED",
                DiagnosticLogStage::UpstreamTls,
            ),
            ("SOCKET_READ_TIMEOUT", DiagnosticLogStage::Socket),
            ("SOCKET_READ_FAILED", DiagnosticLogStage::Socket),
            ("SOCKET_WRITE_TIMEOUT", DiagnosticLogStage::Socket),
            ("SOCKET_WRITE_FAILED", DiagnosticLogStage::Socket),
            ("SOCKET_RELAY_CANCELLED", DiagnosticLogStage::Socket),
            (
                "SOCKET_CONNECTION_TASK_PANICKED",
                DiagnosticLogStage::Socket,
            ),
        ] {
            assert_eq!(
                stage_for_error_code(code),
                expected,
                "unexpected stage for {code}"
            );
        }
        assert_eq!(
            stage_for_error_code("SOCKET_UNKNOWN_FUTURE_CODE"),
            DiagnosticLogStage::System,
            "unknown codes must not be classified by a string prefix"
        );
    }

    #[test]
    fn session_updates_create_http_diagnostics() {
        let payload = UiEventPayload::SessionUpdated(SessionSummaryViewModel {
            session_id: Uuid::nil(),
            request_id: "R-1".into(),
            started_at: Utc::now(),
            completed_at: Some(Utc::now()),
            terminal_ip: "10.0.0.2".into(),
            channel: ChannelId::new("listener-1").expect("valid channel"),
            channel_text: "入口一".into(),
            method: "POST".into(),
            target: "/payment".into(),
            http_status: Some(200),
            result: "成功".into(),
            ui_tone: UiTone::Positive,
            duration_ms: Some(42),
            matched_rule_ids: Vec::new(),
            request_size_bytes: 12,
            response_size_bytes: 24,
            pending_breakpoint: false,
            revision: 1,
        });
        let entry = diagnostic_entry(&payload).expect("HTTP diagnostic event");
        assert_eq!(entry.stage, DiagnosticLogStage::Http);
        assert_eq!(entry.level, DiagnosticLogLevel::Info);
        assert!(
            entry
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("200"))
        );
    }
}
