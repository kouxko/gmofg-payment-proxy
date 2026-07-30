use std::{
    collections::BTreeMap,
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket},
    sync::Arc,
};

use async_trait::async_trait;
use gmofg_proxy_application::{
    AppError, AppResult, FieldValidationViewModel, SettingsDraft, SettingsRepositoryPort,
    SettingsValidationViewModel, SettingsViewModel,
};
use gmofg_proxy_domain::{
    CapacitySettings, ChannelSettings, Revision, Settings, TimeoutSettings, TlsVersion,
};
use parking_lot::RwLock;

use crate::SqliteStore;

use super::common::{infra, json_error};

trait LocalAddressProvider: Debug + Send + Sync {
    fn preferred_lan_ipv4(&self) -> Option<Ipv4Addr>;
}

#[derive(Debug, Default)]
struct SystemLocalAddressProvider;

impl LocalAddressProvider for SystemLocalAddressProvider {
    fn preferred_lan_ipv4(&self) -> Option<Ipv4Addr> {
        // UDP connect only asks the operating system which local interface it
        // would route through. It does not establish a connection or send data.
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
        let IpAddr::V4(address) = socket.local_addr().ok()?.ip() else {
            return None;
        };
        is_usable_lan_ipv4(address).then_some(address)
    }
}

fn is_usable_lan_ipv4(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_multicast()
        && !address.is_broadcast()
}

#[derive(Debug)]
pub struct SettingsRepositoryAdapter {
    store: Arc<SqliteStore>,
    effective: RwLock<Option<SettingsDraft>>,
    local_address: Arc<dyn LocalAddressProvider>,
}

impl SettingsRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self::with_local_address_provider(store, Arc::new(SystemLocalAddressProvider))
    }

    fn with_local_address_provider(
        store: Arc<SqliteStore>,
        local_address: Arc<dyn LocalAddressProvider>,
    ) -> Self {
        Self {
            store,
            effective: RwLock::new(None),
            local_address,
        }
    }

    fn load_stored(&self) -> AppResult<(SettingsDraft, u64)> {
        let (mut draft, revision) = match infra(self.store.load_settings())? {
            Some(stored) => {
                let mut draft: SettingsDraft = serde_json::from_value(stored.value)
                    .map_err(|error| json_error("持久化设置无效", error))?;
                draft.expected_revision = Some(stored.revision);
                (draft, stored.revision)
            }
            None => (SettingsDraft::default(), 0),
        };
        if draft.leaf_sans.is_empty()
            && let Some(address) = self.local_address.preferred_lan_ipv4()
        {
            draft.leaf_sans.push(address.to_string());
        }
        Ok((draft, revision))
    }

    fn view(&self) -> AppResult<SettingsViewModel> {
        let (stored, revision) = self.load_stored()?;
        let effective = self.effective.read().clone();
        let pending_changes = effective.as_ref().is_some_and(|value| value != &stored);
        Ok(SettingsViewModel {
            stored,
            effective,
            pending_changes,
            requires_restart: pending_changes,
            restart_reason: pending_changes
                .then(|| "监听、上游或 TLS 配置变更需要重启 Proxy。".into()),
            revision,
            can_write: true,
            disabled_reason: None,
            fixed_tls_version: "TLS 1.2".into(),
            redirects_enabled: false,
            retries_enabled: false,
            payload_policy_text: "Payload 仅保存在内存中；规则、设置和证书配置持久化。".into(),
        })
    }

    fn validate_domain(draft: &SettingsDraft) -> SettingsValidationViewModel {
        match to_domain_settings(draft).and_then(|settings| settings.validate().map(|()| settings))
        {
            Ok(_) => valid(),
            Err(error) => FieldValidationViewModel {
                valid: false,
                field_errors: error
                    .field_errors
                    .iter()
                    .map(|(field, messages)| (field.clone(), messages.clone()))
                    .collect(),
                warnings: Vec::new(),
            },
        }
    }

    fn validate_ports(&self, draft: &SettingsDraft, validation: &mut SettingsValidationViewModel) {
        let Ok(bind_address) = draft.bind_address.parse::<IpAddr>() else {
            return;
        };
        let effective = self.effective.read();
        for (field, enabled, port, unchanged) in [
            (
                "transaction_port",
                draft.transaction_enabled,
                draft.transaction_port,
                effective.as_ref().is_some_and(|settings| {
                    settings.bind_address == draft.bind_address
                        && settings.transaction_enabled
                        && settings.transaction_port == draft.transaction_port
                }),
            ),
            (
                "dll_port",
                draft.dll_enabled,
                draft.dll_port,
                effective.as_ref().is_some_and(|settings| {
                    settings.bind_address == draft.bind_address
                        && settings.dll_enabled
                        && settings.dll_port == draft.dll_port
                }),
            ),
        ] {
            if enabled
                && !unchanged
                && TcpListener::bind(SocketAddr::new(bind_address, port)).is_err()
            {
                validation.valid = false;
                validation
                    .field_errors
                    .entry(field.into())
                    .or_default()
                    .push(format!("端口 {port} 已被占用或当前用户无权监听。"));
            }
        }
    }
}

#[async_trait]
impl SettingsRepositoryPort for SettingsRepositoryAdapter {
    async fn get(&self) -> AppResult<SettingsViewModel> {
        self.view()
    }

    async fn validate(&self, draft: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        let mut validation = Self::validate_domain(draft);
        if validation.valid {
            self.validate_ports(draft, &mut validation);
        }
        Ok(validation)
    }

    async fn save(&self, mut draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let validation = Self::validate_domain(&draft);
        if !validation.valid {
            return Err(AppError::field(
                "CONFIG_INVALID",
                "设置存在字段错误。",
                validation.field_errors,
            ));
        }
        let expected = draft.expected_revision.unwrap_or(0);
        draft.expected_revision = Some(expected.saturating_add(1));
        let value =
            serde_json::to_value(&draft).map_err(|error| json_error("设置序列化失败", error))?;
        infra(self.store.save_settings(expected, &value))?;
        self.view()
    }

    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel> {
        let (_, current_revision) = self.load_stored()?;
        let mut restored = settings.stored;
        restored.expected_revision = Some(current_revision.saturating_add(1));
        let value = serde_json::to_value(&restored)
            .map_err(|error| json_error("设置回滚序列化失败", error))?;
        infra(self.store.save_settings(current_revision, &value))?;
        *self.effective.write() = settings.effective;
        self.view()
    }

    async fn apply_effective(&self, settings: SettingsDraft) -> AppResult<SettingsViewModel> {
        *self.effective.write() = Some(settings);
        self.view()
    }

    async fn clear_effective(&self) -> AppResult<SettingsViewModel> {
        *self.effective.write() = None;
        self.view()
    }
}

fn valid() -> SettingsValidationViewModel {
    FieldValidationViewModel {
        valid: true,
        field_errors: BTreeMap::default(),
        warnings: Vec::new(),
    }
}

fn to_domain_settings(draft: &SettingsDraft) -> Result<Settings, gmofg_proxy_domain::DomainError> {
    let max_sessions = u32::try_from(draft.max_sessions).map_err(|_| {
        gmofg_proxy_domain::DomainError::new(
            gmofg_proxy_domain::ErrorCode::ConfigInvalid,
            "会话容量超出支持范围",
        )
        .with_field_error("max_sessions", "数值过大")
    })?;
    Ok(Settings {
        revision: Revision::new(draft.expected_revision.unwrap_or(0).max(1)),
        bind_address: draft.bind_address.clone(),
        transaction: ChannelSettings {
            enabled: draft.transaction_enabled,
            port: draft.transaction_port,
            upstream_url: draft.upstream_transaction_url.clone(),
        },
        dll: ChannelSettings {
            enabled: draft.dll_enabled,
            port: draft.dll_port,
            upstream_url: draft.upstream_dll_url.clone(),
        },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FixedLocalAddressProvider(Option<Ipv4Addr>);

    impl LocalAddressProvider for FixedLocalAddressProvider {
        fn preferred_lan_ipv4(&self) -> Option<Ipv4Addr> {
            self.0
        }
    }

    fn adapter_with_address(address: Option<Ipv4Addr>) -> SettingsRepositoryAdapter {
        SettingsRepositoryAdapter::with_local_address_provider(
            Arc::new(SqliteStore::in_memory().expect("store")),
            Arc::new(FixedLocalAddressProvider(address)),
        )
    }

    fn valid_draft() -> SettingsDraft {
        SettingsDraft {
            upstream_transaction_url: "https://transaction.example.test".into(),
            upstream_dll_url: "https://dll.example.test".into(),
            ..SettingsDraft::default()
        }
    }

    #[tokio::test]
    async fn initial_settings_autofill_the_preferred_lan_ipv4_as_leaf_san() {
        let adapter = adapter_with_address(Some(Ipv4Addr::new(10, 0, 34, 50)));

        let settings = adapter.get().await.expect("settings");

        assert_eq!(settings.stored.leaf_sans, vec!["10.0.34.50"]);
        assert_eq!(settings.revision, 0);
    }

    #[tokio::test]
    async fn stored_leaf_san_is_never_overwritten_by_address_detection() {
        let adapter = adapter_with_address(Some(Ipv4Addr::new(10, 0, 34, 50)));
        let mut draft = valid_draft();
        draft.leaf_sans = vec!["10.0.28.99".into()];
        adapter.save(draft).await.expect("save");

        let settings = adapter.get().await.expect("settings");

        assert_eq!(settings.stored.leaf_sans, vec!["10.0.28.99"]);
        assert_eq!(settings.revision, 1);
    }

    #[tokio::test]
    async fn unavailable_address_detection_keeps_the_leaf_san_empty() {
        let adapter = adapter_with_address(None);

        let settings = adapter.get().await.expect("settings");

        assert!(settings.stored.leaf_sans.is_empty());
    }

    // SETTINGS-010~013, ENGINE-008, TEST-SETTINGS
    #[tokio::test]
    async fn persists_revision_and_tracks_effective_snapshot_separately() {
        let adapter =
            SettingsRepositoryAdapter::new(Arc::new(SqliteStore::in_memory().expect("store")));
        let saved = adapter.save(valid_draft()).await.expect("save");
        assert_eq!(saved.revision, 1);
        assert!(saved.effective.is_none());
        let applied = adapter
            .apply_effective(saved.stored.clone())
            .await
            .expect("apply");
        assert!(!applied.requires_restart);

        let mut changed = applied.stored;
        changed.transaction_port = 20_000;
        let changed = adapter.save(changed).await.expect("change");
        assert!(changed.requires_restart);
        assert_eq!(
            changed.effective.expect("effective").transaction_port,
            16_627
        );
    }

    #[tokio::test]
    async fn validation_reports_an_occupied_listener_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve port");
        let port = listener.local_addr().expect("local address").port();
        let adapter =
            SettingsRepositoryAdapter::new(Arc::new(SqliteStore::in_memory().expect("store")));
        let draft = SettingsDraft {
            bind_address: "127.0.0.1".into(),
            transaction_port: port,
            dll_enabled: false,
            leaf_sans: vec!["127.0.0.1".into()],
            ..valid_draft()
        };
        let validation = adapter.validate(&draft).await.expect("validation");
        assert!(!validation.valid);
        assert!(validation.field_errors.contains_key("transaction_port"));
    }
}
