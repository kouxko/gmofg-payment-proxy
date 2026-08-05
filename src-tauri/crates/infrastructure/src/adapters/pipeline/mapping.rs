use super::{
    AppChannelId, AppError, AppMessageStage, BodyCodec, BreakpointDecision, BreakpointDecisionKind,
    BreakpointDetailViewModel, BreakpointState, BreakpointSummaryViewModel, Bytes, ChannelId,
    ConnectionContext, DomainChannelId, Duration, ErrorCode, FaultAction, Message, ProxyError,
    ProxyResult, UiTone, Utc, Uuid, content_view, encode_body, message_method, message_target,
    proxy_message,
};
#[cfg(test)]
use super::{DomainMessageStage, Rule};

pub(super) fn apply_breakpoint_decision(
    body_codec: &dyn BodyCodec,
    stage: AppMessageStage,
    original: &Message,
    effective: &mut Message,
    decision: &BreakpointDecision,
) -> ProxyResult<Vec<FaultAction>> {
    let mut actions = Vec::new();
    match decision.kind {
        BreakpointDecisionKind::ForwardOriginal => *effective = original.clone(),
        BreakpointDecisionKind::ForwardModified => {
            *effective = proxy_message(
                decision.message.as_ref().ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "modified message is missing")
                })?,
                &effective.start_line,
            )?;
        }
        BreakpointDecisionKind::MockResponse => {
            let message = decision.message.as_ref().ok_or_else(|| {
                ProxyError::new(ErrorCode::ConfigInvalid, "mock response is missing")
            })?;
            let mock = proxy_message(message, "HTTP/1.1 200 OK")?;
            actions.push(FaultAction::MockResponse {
                status: proxy_status!(decision.http_status.unwrap_or(200))?,
                headers: mock.header_map()?,
                body: Bytes::from(encode_body(
                    body_codec,
                    message.body_text.as_deref().ok_or_else(|| ProxyError {
                        code: "BODY_ENCODE_FAILED",
                        message: "mock body text is missing".into(),
                    })?,
                )?),
            });
        }
        BreakpointDecisionKind::Delay => {
            actions.push(FaultAction::Delay(Duration::from_millis(
                decision
                    .delay_ms
                    .ok_or_else(|| ProxyError::new(ErrorCode::ConfigInvalid, "delay is missing"))?,
            )));
        }
        BreakpointDecisionKind::DisconnectBeforeUpstream => {
            actions.push(FaultAction::DisconnectBeforeUpstream);
        }
        BreakpointDecisionKind::CustomHttpStatus => {
            actions.push(FaultAction::CustomStatus(proxy_status!(
                decision.http_status.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "HTTP status is missing")
                })?
            )?));
        }
        BreakpointDecisionKind::InvalidJson => actions.push(FaultAction::ReplaceBody {
            body: Bytes::from(encode_body(body_codec, "{invalid-json")?),
        }),
        BreakpointDecisionKind::WrongContentLength => {
            actions.push(FaultAction::ContentLengthOffset(
                decision.content_length_delta.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "content-length delta is missing")
                })?,
            ));
        }
        BreakpointDecisionKind::Truncate => {
            actions.push(FaultAction::TruncateResponse(
                decision.truncate_at.ok_or_else(|| {
                    ProxyError::new(ErrorCode::ConfigInvalid, "truncate position is missing")
                })?,
            ));
        }
        BreakpointDecisionKind::DropResponse => {
            actions.push(FaultAction::DropResponse {
                read_upstream: stage == AppMessageStage::Request,
            });
        }
    }
    Ok(actions)
}

#[cfg(test)]
pub(super) fn view_to_domain_rule(
    view: intercept_proxy_application::RuleViewModel,
) -> ProxyResult<Rule> {
    let draft = view.draft;
    let stage = match draft.stage {
        Some(AppMessageStage::Request) => DomainMessageStage::Request,
        Some(AppMessageStage::Response) => DomainMessageStage::Response,
        Some(AppMessageStage::TlsHandshake) => DomainMessageStage::TlsHandshake,
        _ => {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "rule has an invalid stage",
            ));
        }
    };
    let conditions = draft
        .conditions
        .iter()
        .map(crate::adapters::rules::condition_to_domain)
        .collect();
    let actions = draft
        .actions
        .iter()
        .map(crate::adapters::rules::action_to_domain)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| ProxyError::new(ErrorCode::ConfigInvalid, error.to_string()))?;
    Ok(Rule {
        id: intercept_proxy_domain::RuleId::from_uuid(view.summary.rule_id),
        revision: intercept_proxy_domain::Revision::new(view.summary.revision),
        name: draft.name,
        description: draft.description,
        enabled: draft.enabled,
        priority: u32::try_from(draft.priority).map_err(|_| {
            ProxyError::new(ErrorCode::ConfigInvalid, "rule priority cannot be negative")
        })?,
        created_order: view.summary.creation_order,
        channel: draft.channel,
        stage,
        conditions,
        actions,
        one_shot: draft.one_shot,
        hit_count: view.summary.hit_count,
        last_hit_at: view.summary.last_hit_at,
    })
}

pub(super) fn breakpoint_detail(
    body_codec: &dyn BodyCodec,
    context: &ConnectionContext,
    channel_text: String,
    stage: AppMessageStage,
    original: &Message,
    effective: &Message,
    session_id: Uuid,
) -> ProxyResult<BreakpointDetailViewModel> {
    let title = match stage {
        AppMessageStage::Request => "请求断点·发送至服务器前",
        AppMessageStage::Response => "响应断点·返回 App 前",
        AppMessageStage::TlsHandshake | AppMessageStage::Terminal => "终态",
    };
    Ok(BreakpointDetailViewModel {
        summary: BreakpointSummaryViewModel {
            breakpoint_id: Uuid::new_v4(),
            session_id,
            runtime_epoch: context.runtime_epoch,
            stage,
            title: title.into(),
            terminal_ip: context.peer_addr.ip().to_string(),
            channel: app_channel(&context.channel)?,
            channel_text,
            method: message_method(&effective.start_line)
                .unwrap_or_default()
                .to_owned(),
            target: message_target(&effective.start_line)
                .unwrap_or_default()
                .to_owned(),
            waiting_since: Utc::now(),
            certificate_fingerprint_suffix: fingerprint_suffix(&fingerprint(context)),
            state: BreakpointState::Pending,
            state_text: "待处理".into(),
            ui_tone: UiTone::Warning,
            revision: 1,
        },
        original: content_view(body_codec, original),
        effective: content_view(body_codec, effective),
        can_resolve: true,
        resolve_disabled_reason: None,
        available_actions: Vec::new(),
    })
}

pub(super) fn app_to_proxy(error: AppError) -> ProxyError {
    if matches!(
        error.view_model.code.as_str(),
        "RESOURCE_EXHAUSTED" | "REVISION_CONFLICT"
    ) {
        let code = if error.view_model.code == "RESOURCE_EXHAUSTED" {
            "RESOURCE_EXHAUSTED"
        } else {
            "REVISION_CONFLICT"
        };
        return ProxyError {
            code,
            message: error.view_model.message,
        };
    }
    if matches!(
        error.view_model.code.as_str(),
        "BODY_DECODE_FAILED" | "BODY_ENCODE_FAILED"
    ) {
        return ProxyError {
            code: if error.view_model.code == "BODY_DECODE_FAILED" {
                "BODY_DECODE_FAILED"
            } else {
                "BODY_ENCODE_FAILED"
            },
            message: error.view_model.message,
        };
    }
    let code = match error.view_model.code.as_str() {
        "JSON_INVALID" | "CONFIG_INVALID" | "RULE_INVALID" => ErrorCode::ConfigInvalid,
        _ => ErrorCode::Internal,
    };
    ProxyError::new(code, error.view_model.message)
}

pub(super) fn app_channel(channel: &ChannelId) -> ProxyResult<AppChannelId> {
    AppChannelId::new(channel.as_str()).map_err(|error| {
        ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!("invalid application channel `{channel}`: {error}"),
        )
    })
}

pub(super) fn domain_channel(channel: &ChannelId) -> ProxyResult<DomainChannelId> {
    DomainChannelId::new(channel.as_str()).map_err(|error| {
        ProxyError::new(
            ErrorCode::ConfigInvalid,
            format!("invalid domain channel `{channel}`: {error}"),
        )
    })
}

pub(super) fn fingerprint(context: &ConnectionContext) -> String {
    context
        .tls_peer
        .as_ref()
        .map_or_else(String::new, |identity| identity.sha256_fingerprint.clone())
}

pub(super) fn fingerprint_suffix(fingerprint: &str) -> String {
    fingerprint
        .chars()
        .rev()
        .take(12)
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}

pub(super) fn tls_summary(context: &ConnectionContext) -> String {
    context.tls_peer.as_ref().map_or_else(
        || "未记录下游客户端证书（可能为明文或未启用 mTLS）".into(),
        |identity| format!("已验证下游客户端证书 / {}", identity.subject_summary),
    )
}

pub(super) fn result_text(code: &str) -> &'static str {
    match code {
        "UPSTREAM_CONNECT_TIMEOUT" | "UPSTREAM_WRITE_TIMEOUT" | "UPSTREAM_READ_TIMEOUT" => {
            "上游超时"
        }
        "BREAKPOINT_CLIENT_DISCONNECTED" => "App 断开",
        "BREAKPOINT_PROXY_STOPPED" | "FAULT_EXECUTION_CANCELLED" => "Proxy 停止",
        "TLS_HANDSHAKE_FAILED" => "TLS 失败",
        "INCORRECT_CONTENT_LENGTH" => "规则终止",
        "TRUNCATED_RESPONSE" => "截断",
        "FAULT_STREAM_ABORTED" => "弱网断连",
        _ => "内部错误",
    }
}

pub(super) fn result_tone(code: &str) -> UiTone {
    match code {
        "BREAKPOINT_PROXY_STOPPED" => UiTone::Warning,
        _ => UiTone::Danger,
    }
}
