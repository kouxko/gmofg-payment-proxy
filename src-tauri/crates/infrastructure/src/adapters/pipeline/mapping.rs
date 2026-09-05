use super::{
    AppChannelId, AppError, ChannelId, ConnectionContext, DomainChannelId, ErrorCode, ProxyError,
    ProxyResult,
};

pub(crate) fn app_to_proxy(error: AppError) -> ProxyError {
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
            external_package_call: None,
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
            external_package_call: None,
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

pub(super) fn tls_summary(context: &ConnectionContext) -> String {
    context.tls_peer.as_ref().map_or_else(
        || "未记录下游客户端证书（可能为明文或未启用 mTLS）".into(),
        |identity| format!("已验证下游客户端证书 / {}", identity.subject_summary),
    )
}
