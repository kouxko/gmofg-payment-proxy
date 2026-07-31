use crate::{ChannelId, DomainError, ErrorCode, Revision};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::net::IpAddr;

pub const DEFAULT_TIMEOUT_MS: u64 = 70_000;
pub const DEFAULT_MAX_BODY_BYTES: u64 = 4 * 1024 * 1024;
pub const DEFAULT_MAX_SESSIONS: u32 = 500;
pub const DEFAULT_MAX_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
pub const DEFAULT_UI_EVENT_CAPACITY: u32 = 4_096;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub enum TlsVersion {
    Tls12,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct ChannelSettings {
    pub id: ChannelId,
    pub enabled: bool,
    pub port: u16,
    pub upstream_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct TimeoutSettings {
    pub connect_ms: u64,
    pub write_ms: u64,
    pub read_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct CapacitySettings {
    pub max_sessions: u32,
    pub max_memory_bytes: u64,
    pub max_body_bytes: u64,
    pub ui_event_capacity: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct Settings {
    pub revision: Revision,
    pub bind_address: String,
    pub channels: Vec<ChannelSettings>,
    pub tls_version: TlsVersion,
    pub follow_redirects: bool,
    pub automatic_retries: bool,
    pub rewrite_host: bool,
    pub timeouts: TimeoutSettings,
    pub capacity: CapacitySettings,
    pub leaf_certificate_sans: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: Revision::INITIAL,
            bind_address: "0.0.0.0".into(),
            channels: Vec::new(),
            tls_version: TlsVersion::Tls12,
            follow_redirects: false,
            automatic_retries: false,
            rewrite_host: true,
            timeouts: TimeoutSettings {
                connect_ms: DEFAULT_TIMEOUT_MS,
                write_ms: DEFAULT_TIMEOUT_MS,
                read_ms: DEFAULT_TIMEOUT_MS,
            },
            capacity: CapacitySettings {
                max_sessions: DEFAULT_MAX_SESSIONS,
                max_memory_bytes: DEFAULT_MAX_MEMORY_BYTES,
                max_body_bytes: DEFAULT_MAX_BODY_BYTES,
                ui_event_capacity: DEFAULT_UI_EVENT_CAPACITY,
            },
            leaf_certificate_sans: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct SettingsDraft {
    pub expected_revision: Revision,
    pub values: Settings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct SettingsSnapshot {
    pub stored: Settings,
    pub effective: Option<Settings>,
    pub restart_required: bool,
}

impl Settings {
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut error = DomainError::new(ErrorCode::ConfigInvalid, "设置存在字段错误");
        let bind_ip = self.bind_address.parse::<IpAddr>();
        if bind_ip.is_err() {
            error = error.with_field_error("bind_address", "绑定地址必须是有效 IP 地址");
        }
        if !self.channels.iter().any(|channel| channel.enabled) {
            error = error.with_field_error("channels", "至少启用一个通道");
        }
        let mut channel_ids = std::collections::BTreeSet::new();
        let mut ports = std::collections::BTreeMap::new();
        for channel in &self.channels {
            if !channel_ids.insert(channel.id.as_str()) {
                error = error.with_field_error(
                    format!("channels.{}.id", channel.id.as_str()),
                    "通道 ID 不能重复",
                );
            }
            if channel.enabled
                && let Some(existing) = ports.insert(channel.port, channel.id.as_str())
            {
                error = error.with_field_error(
                    format!("channels.{}.port", channel.id.as_str()),
                    format!("监听端口与通道 {existing} 重复"),
                );
            }
            validate_channel(channel, &mut error);
        }
        if self.follow_redirects {
            error = error.with_field_error("follow_redirects", "HTTP 重定向固定关闭");
        }
        if self.automatic_retries {
            error = error.with_field_error("automatic_retries", "自动重试固定关闭");
        }
        if self.timeouts.connect_ms == 0
            || self.timeouts.write_ms == 0
            || self.timeouts.read_ms == 0
        {
            error = error.with_field_error("timeouts", "超时必须大于 0 毫秒");
        }
        if self.capacity.max_sessions == 0
            || self.capacity.max_memory_bytes == 0
            || self.capacity.max_body_bytes == 0
            || self.capacity.ui_event_capacity == 0
        {
            error = error.with_field_error("capacity", "容量限制必须大于 0");
        }
        if let Ok(bind_ip) = bind_ip
            && !bind_ip.is_unspecified()
            && !self
                .leaf_certificate_sans
                .iter()
                .any(|san| san == &self.bind_address)
        {
            error = error.with_field_error(
                "leaf_certificate_sans",
                "叶子证书 SAN 必须包含非通配绑定 IP",
            );
        }
        if error.field_errors.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    pub fn apply_draft(&mut self, draft: SettingsDraft) -> Result<Revision, DomainError> {
        self.revision.verify(draft.expected_revision)?;
        draft.values.validate()?;
        let next = self.revision.next();
        *self = draft.values;
        self.revision = next;
        Ok(next)
    }
}

fn validate_channel(channel: &ChannelSettings, error: &mut DomainError) {
    if !channel.enabled {
        return;
    }
    let prefix = format!("channels.{}", channel.id.as_str());
    if channel.port == 0 {
        *error = std::mem::replace(
            error,
            DomainError::new(ErrorCode::ConfigInvalid, "设置存在字段错误"),
        )
        .with_field_error(format!("{prefix}.port"), "端口必须大于 0");
    }
    let url = channel.upstream_url.trim();
    if !is_valid_https_upstream_url(url) {
        *error = std::mem::replace(
            error,
            DomainError::new(ErrorCode::ConfigInvalid, "设置存在字段错误"),
        )
        .with_field_error(format!("{prefix}.upstream_url"), "上游 URL 非法");
    }
}

#[must_use]
pub fn is_valid_https_upstream_url(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }

    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let host = &bracketed[..end];
        let suffix = &bracketed[end + 1..];
        if !valid_optional_port(suffix) {
            return false;
        }
        return host
            .parse::<IpAddr>()
            .is_ok_and(|address| matches!(address, IpAddr::V6(_)));
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };

    host.parse::<IpAddr>().is_ok() || valid_dns_name(host)
}

fn valid_optional_port(value: &str) -> bool {
    value.is_empty() || value.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

fn valid_dns_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_settings() -> Settings {
        Settings {
            channels: vec![
                ChannelSettings {
                    id: ChannelId::new("alpha").unwrap(),
                    enabled: true,
                    port: 20_001,
                    upstream_url: "https://alpha.example.test/api".into(),
                },
                ChannelSettings {
                    id: ChannelId::new("beta").unwrap(),
                    enabled: true,
                    port: 20_002,
                    upstream_url: "https://beta.example.test/api".into(),
                },
            ],
            ..Settings::default()
        }
    }

    // SETTINGS-002, SETTINGS-003, SETTINGS-004, SETTINGS-005, SETTINGS-007, SETTINGS-008
    #[test]
    fn defaults_are_frozen_and_safe() {
        let defaults = Settings::default();
        assert_eq!(defaults.bind_address, "0.0.0.0");
        assert!(defaults.channels.is_empty());
        assert_eq!(defaults.tls_version, TlsVersion::Tls12);
        assert!(!defaults.follow_redirects);
        assert!(!defaults.automatic_retries);
        assert_eq!(defaults.timeouts.connect_ms, 70_000);
        assert_eq!(defaults.capacity.max_sessions, 500);
        assert_eq!(defaults.capacity.max_memory_bytes, 256 * 1024 * 1024);
    }

    // STATE-005, SETTINGS-012, TEST-SETTINGS
    #[test]
    fn validates_channels_ports_urls_and_certificate_san() {
        let mut settings = valid_settings();
        settings.bind_address = "192.168.1.10".into();
        settings.channels[1].port = settings.channels[0].port;
        let error = settings.validate().unwrap_err();
        assert!(error.field_errors.contains_key("channels.beta.port"));
        assert!(error.field_errors.contains_key("leaf_certificate_sans"));
    }

    // PROXY-001, SETTINGS-012: production upstream transport is HTTPS only.
    #[test]
    fn rejects_plain_http_upstream_urls() {
        let mut settings = valid_settings();
        settings.channels[0].upstream_url = "http://alpha.example.test/api".into();
        let error = settings.validate().expect_err("http must be rejected");
        assert!(
            error
                .field_errors
                .contains_key("channels.alpha.upstream_url")
        );
    }

    #[test]
    fn rejects_https_urls_without_a_valid_authority() {
        for invalid in [
            "https://?query",
            "https:///missing-host",
            "https://user@example.test",
            "https://bad host.test",
            "https://example.test:0",
            "https://-invalid.example.test",
        ] {
            assert!(
                !is_valid_https_upstream_url(invalid),
                "{invalid} must be rejected"
            );
        }
        assert!(is_valid_https_upstream_url(
            "https://transaction.example.test:443/api?mode=test"
        ));
        assert!(is_valid_https_upstream_url("https://[::1]:8443/api"));
    }

    // ENGINE-008, SETTINGS-010
    #[test]
    fn settings_apply_uses_optimistic_revision_without_changing_effective_snapshot() {
        let mut stored = valid_settings();
        let effective = stored.clone();
        let mut candidate = valid_settings();
        candidate.channels[0].port = 20_003;
        let revision = stored
            .apply_draft(SettingsDraft {
                expected_revision: Revision::INITIAL,
                values: candidate,
            })
            .unwrap();
        let snapshot = SettingsSnapshot {
            restart_required: stored != effective,
            stored,
            effective: Some(effective),
        };
        assert_eq!(revision, Revision::new(2));
        assert_eq!(snapshot.effective.unwrap().revision, Revision::INITIAL);
    }
}
