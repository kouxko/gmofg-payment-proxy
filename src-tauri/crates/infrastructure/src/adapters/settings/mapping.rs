use super::{
    BTreeMap, CapacitySettings, ChannelSettings, ChannelSettingsDraft, FieldValidationViewModel,
    ProductProfile, Revision, Settings, SettingsDraft, SettingsValidationViewModel,
    TimeoutSettings, TlsVersion,
};

pub(super) fn valid() -> SettingsValidationViewModel {
    FieldValidationViewModel {
        valid: true,
        field_errors: BTreeMap::default(),
        warnings: Vec::new(),
    }
}

pub(super) fn to_domain_settings(
    draft: &SettingsDraft,
) -> Result<Settings, intercept_proxy_domain::DomainError> {
    let max_sessions = u32::try_from(draft.max_sessions).map_err(|_| {
        intercept_proxy_domain::DomainError::new(
            intercept_proxy_domain::ErrorCode::ConfigInvalid,
            "会话容量超出支持范围",
        )
        .with_field_error("max_sessions", "数值过大")
    })?;
    Ok(Settings {
        revision: Revision::new(draft.expected_revision.unwrap_or(0).max(1)),
        bind_address: draft.bind_address.clone(),
        channels: draft
            .channels
            .iter()
            .map(|channel| ChannelSettings {
                id: channel.id.clone(),
                enabled: channel.enabled,
                port: channel.port,
                upstream_url: channel.upstream_url.clone(),
            })
            .collect(),
        tls_version: TlsVersion::Tls12,
        follow_redirects: false,
        automatic_retries: false,
        rewrite_host: draft.rewrite_host,
        timeouts: TimeoutSettings {
            connect_ms: draft.connect_timeout_seconds.saturating_mul(1_000),
            write_ms: draft.write_timeout_seconds.saturating_mul(1_000),
            read_ms: draft.read_timeout_seconds.saturating_mul(1_000),
        },
        capacity: CapacitySettings {
            max_sessions,
            max_memory_bytes: draft.max_memory_bytes,
            max_body_bytes: draft.max_body_bytes,
            ui_event_capacity: 4_096,
        },
        leaf_certificate_sans: draft.leaf_sans.clone(),
    })
}

pub(super) fn default_settings(product: &dyn ProductProfile) -> SettingsDraft {
    SettingsDraft {
        channels: product
            .channels()
            .iter()
            .map(|channel| ChannelSettingsDraft {
                id: intercept_proxy_domain::ChannelId::new(channel.id)
                    .expect("product channel IDs are compile-time validated"),
                display_name: channel.display_name.into(),
                enabled: channel.enabled_by_default,
                port: channel.listen_port,
                upstream_url: channel.upstream_url.into(),
            })
            .collect(),
        ..SettingsDraft::default()
    }
}
