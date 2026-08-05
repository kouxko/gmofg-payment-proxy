use super::{
    AppError, AppResult, ApplicationProxyState, ChannelConfig, ChannelState,
    DEFAULT_MAX_CONNECTIONS, DisabledReason, Duration, IpAddr, MessageLimits, ProxyConfig,
    ProxyError, ProxyState, RuntimeChannelId, SettingsDraft, SocketAddr, UiTone,
};

pub(super) fn proxy_config(settings: &SettingsDraft) -> AppResult<ProxyConfig> {
    let bind_address = settings
        .bind_address
        .parse::<IpAddr>()
        .map_err(|error| AppError::new("CONFIG_INVALID", format!("绑定地址无效: {error}")))?;
    let max_body_bytes = usize::try_from(settings.max_body_bytes)
        .map_err(|_| AppError::new("CONFIG_INVALID", "Body 上限超出平台范围"))?;
    Ok(ProxyConfig {
        channels: settings
            .channels
            .iter()
            .map(|channel| {
                Ok(ChannelConfig {
                    channel: RuntimeChannelId::new(channel.id.as_str()).map_err(map_error)?,
                    enabled: channel.enabled,
                    listen_addr: SocketAddr::new(bind_address, channel.port),
                    upstream_url: channel.upstream_url.clone(),
                })
            })
            .collect::<AppResult<Vec<_>>>()?,
        limits: MessageLimits {
            max_body_bytes,
            ..MessageLimits::default()
        },
        max_connections: DEFAULT_MAX_CONNECTIONS,
        connect_timeout: Duration::from_secs(settings.connect_timeout_seconds),
        write_timeout: Duration::from_secs(settings.write_timeout_seconds),
        read_timeout: Duration::from_secs(settings.read_timeout_seconds),
        rewrite_host: settings.rewrite_host,
        leaf_sans: settings.leaf_sans.clone(),
    })
}

pub(super) fn map_state(state: ProxyState) -> (ApplicationProxyState, String, UiTone) {
    let state = match state {
        ProxyState::Stopped => ApplicationProxyState::Stopped,
        ProxyState::Starting => ApplicationProxyState::Starting,
        ProxyState::Running => ApplicationProxyState::Running,
        ProxyState::Stopping => ApplicationProxyState::Stopping,
        ProxyState::Faulted => ApplicationProxyState::Faulted,
    };
    let (text, tone) = state.display_zh();
    (state, text.to_owned(), tone)
}

pub(super) fn map_channel_state(state: ProxyState, enabled: bool) -> ChannelState {
    if !enabled {
        return ChannelState::Disabled;
    }
    match state {
        ProxyState::Stopped => ChannelState::Stopped,
        ProxyState::Starting => ChannelState::Starting,
        ProxyState::Running => ChannelState::Listening,
        ProxyState::Stopping => ChannelState::Stopping,
        ProxyState::Faulted => ChannelState::Faulted,
    }
}

pub(super) fn disabled_reason(disabled: bool, message: &str) -> Option<DisabledReason> {
    disabled.then(|| DisabledReason {
        code: "OPERATION_NOT_ALLOWED".to_owned(),
        message: message.to_owned(),
    })
}

pub(super) fn map_error(error: ProxyError) -> AppError {
    AppError::new(error.code, error.message)
}
