//! 设置仓库适配器：负责默认值、字段校验、本机地址探测与 `SQLite` 版本更新。
//!
//! 端口可用性检查只是保存前提示，真正启动仍可能因竞态绑定失败；设置写入使用 revision
//! 防止旧页面覆盖新值，网络探测失败则回退为无建议而不是阻止应用启动。

use std::{
    collections::BTreeMap,
    fmt::Debug,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, UdpSocket},
    sync::Arc,
};

use async_trait::async_trait;
use gmofg_proxy_application::{
    AppError, AppResult, ChannelSettingsDraft, FieldValidationViewModel, SettingsDraft,
    SettingsRepositoryPort, SettingsValidationViewModel, SettingsViewModel,
};
use gmofg_proxy_domain::{
    CapacitySettings, ChannelSettings, Revision, Settings, TimeoutSettings, TlsVersion,
};
use gmofg_proxy_product_api::{LegacySettingsChannelMapping, ProductProfile};
use parking_lot::RwLock;
use serde_json::{Map, Value};

use crate::SqliteStore;

use super::common::{infra, json_error};

const PERSISTENCE_VERSION_FIELD: &str = "_persistence_version";
const SETTINGS_PERSISTENCE_VERSION: u64 = 1;

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
    defaults: SettingsDraft,
    legacy_settings_channels: &'static [LegacySettingsChannelMapping],
    effective: RwLock<Option<SettingsDraft>>,
    local_address: Arc<dyn LocalAddressProvider>,
}

impl SettingsRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>, product: &dyn ProductProfile) -> Self {
        Self::with_local_address_provider(
            store,
            default_settings(product),
            product.persistence_migrations().settings_channels,
            Arc::new(SystemLocalAddressProvider),
        )
    }

    fn with_local_address_provider(
        store: Arc<SqliteStore>,
        defaults: SettingsDraft,
        legacy_settings_channels: &'static [LegacySettingsChannelMapping],
        local_address: Arc<dyn LocalAddressProvider>,
    ) -> Self {
        Self {
            store,
            defaults,
            legacy_settings_channels,
            effective: RwLock::new(None),
            local_address,
        }
    }

    fn load_stored(&self) -> AppResult<(SettingsDraft, u64)> {
        let (mut draft, revision) = match infra(self.store.load_settings())? {
            Some(stored) => {
                let mut draft = deserialize_settings(
                    stored.value,
                    &self.defaults,
                    self.legacy_settings_channels,
                )
                .map_err(|error| json_error("持久化设置无效", error))?;
                self.canonicalize_catalog(&mut draft)?;
                draft.expected_revision = Some(stored.revision);
                (draft, stored.revision)
            }
            None => (self.defaults.clone(), 0),
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
        for channel in &draft.channels {
            let unchanged = effective.as_ref().is_some_and(|settings| {
                settings.bind_address == draft.bind_address
                    && settings.channels.iter().any(|effective_channel| {
                        effective_channel.id == channel.id
                            && effective_channel.enabled
                            && effective_channel.port == channel.port
                    })
            });
            if channel.enabled
                && !unchanged
                && TcpListener::bind(SocketAddr::new(bind_address, channel.port)).is_err()
            {
                validation.valid = false;
                validation
                    .field_errors
                    .entry(format!("channels.{}.port", channel.id.as_str()))
                    .or_default()
                    .push(format!(
                        "端口 {} 已被占用或当前用户无权监听。",
                        channel.port
                    ));
            }
        }
    }

    fn validate_catalog(
        &self,
        draft: &SettingsDraft,
        validation: &mut SettingsValidationViewModel,
    ) {
        for expected in &self.defaults.channels {
            match draft
                .channels
                .iter()
                .find(|channel| channel.id == expected.id)
            {
                None => {
                    validation.valid = false;
                    validation
                        .field_errors
                        .entry("channels".into())
                        .or_default()
                        .push(format!("缺少产品通道 {}。", expected.id));
                }
                Some(channel) if channel.display_name != expected.display_name => {
                    validation.valid = false;
                    validation
                        .field_errors
                        .entry(format!("channels.{}.display_name", channel.id))
                        .or_default()
                        .push("通道显示名由产品配置固定提供。".into());
                }
                Some(_) => {}
            }
        }
        for channel in &draft.channels {
            if !self
                .defaults
                .channels
                .iter()
                .any(|expected| expected.id == channel.id)
            {
                validation.valid = false;
                validation
                    .field_errors
                    .entry(format!("channels.{}.id", channel.id))
                    .or_default()
                    .push("产品未声明该通道。".into());
            }
        }
    }

    fn canonicalize_catalog(&self, draft: &mut SettingsDraft) -> AppResult<()> {
        let mut validation = valid();
        self.validate_catalog(draft, &mut validation);
        if !validation.valid {
            return Err(AppError::field(
                "PERSISTENCE_CORRUPT",
                "持久化设置中的通道目录与当前产品不兼容。",
                validation.field_errors,
            ));
        }
        for channel in &mut draft.channels {
            let expected = self
                .defaults
                .channels
                .iter()
                .find(|expected| expected.id == channel.id)
                .expect("catalog validated");
            channel.display_name.clone_from(&expected.display_name);
        }
        Ok(())
    }
}

#[async_trait]
impl SettingsRepositoryPort for SettingsRepositoryAdapter {
    async fn defaults(&self) -> AppResult<SettingsDraft> {
        Ok(self.defaults.clone())
    }

    async fn get(&self) -> AppResult<SettingsViewModel> {
        self.view()
    }

    async fn validate(&self, draft: &SettingsDraft) -> AppResult<SettingsValidationViewModel> {
        let mut validation = Self::validate_domain(draft);
        self.validate_catalog(draft, &mut validation);
        if validation.valid {
            self.validate_ports(draft, &mut validation);
        }
        Ok(validation)
    }

    async fn save(&self, mut draft: SettingsDraft) -> AppResult<SettingsViewModel> {
        let mut validation = Self::validate_domain(&draft);
        self.validate_catalog(&draft, &mut validation);
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
            serialize_settings(&draft).map_err(|error| json_error("设置序列化失败", error))?;
        infra(self.store.save_settings(expected, &value))?;
        self.view()
    }

    async fn restore(&self, settings: SettingsViewModel) -> AppResult<SettingsViewModel> {
        let (_, current_revision) = self.load_stored()?;
        let mut restored = settings.stored;
        restored.expected_revision = Some(current_revision.saturating_add(1));
        let value = serialize_settings(&restored)
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

fn serialize_settings(draft: &SettingsDraft) -> Result<Value, serde_json::Error> {
    let mut value = serde_json::to_value(draft)?;
    value
        .as_object_mut()
        .expect("SettingsDraft always serializes as an object")
        .insert(
            PERSISTENCE_VERSION_FIELD.into(),
            Value::from(SETTINGS_PERSISTENCE_VERSION),
        );
    Ok(value)
}

fn deserialize_settings(
    mut value: Value,
    defaults: &SettingsDraft,
    legacy_channels: &[LegacySettingsChannelMapping],
) -> Result<SettingsDraft, serde_json::Error> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| serde::de::Error::custom("settings root must be an object"))?;
    let version = take_persistence_version(object)?;
    let has_channels = object.contains_key("channels");
    match (version, has_channels) {
        (Some(SETTINGS_PERSISTENCE_VERSION) | None, true) => serde_json::from_value(value),
        (None, false) if !legacy_channels.is_empty() => {
            migrate_legacy_channel_settings(value, defaults, legacy_channels)
        }
        (Some(version), _) => Err(serde::de::Error::custom(format!(
            "unsupported settings persistence version {version}"
        ))),
        _ => serde_json::from_value(value),
    }
}

fn take_persistence_version(
    object: &mut Map<String, Value>,
) -> Result<Option<u64>, serde_json::Error> {
    object
        .remove(PERSISTENCE_VERSION_FIELD)
        .map(|value| {
            value.as_u64().ok_or_else(|| {
                serde::de::Error::custom("persistence version must be an unsigned integer")
            })
        })
        .transpose()
}

fn migrate_legacy_channel_settings(
    mut value: Value,
    defaults: &SettingsDraft,
    mappings: &[LegacySettingsChannelMapping],
) -> Result<SettingsDraft, serde_json::Error> {
    let object = value
        .as_object_mut()
        .expect("settings root was validated before migration");
    let mut channels = Vec::with_capacity(mappings.len());
    for mapping in mappings {
        let expected = defaults
            .channels
            .iter()
            .find(|channel| channel.id.as_str() == mapping.channel_id)
            .ok_or_else(|| {
                serde::de::Error::custom(format!(
                    "product profile does not declare legacy channel {}",
                    mapping.channel_id
                ))
            })?;
        channels.push(serde_json::json!({
            "id": expected.id,
            "display_name": expected.display_name,
            "enabled": take_legacy_field(object, mapping.enabled_field)?,
            "port": take_legacy_field(object, mapping.port_field)?,
            "upstream_url": take_legacy_field(object, mapping.upstream_url_field)?,
        }));
    }
    object.insert("channels".into(), Value::Array(channels));
    serde_json::from_value(value)
}

fn take_legacy_field(
    object: &mut Map<String, Value>,
    field: &str,
) -> Result<Value, serde_json::Error> {
    object.remove(field).ok_or_else(|| {
        serde::de::Error::custom(format!("legacy settings field {field:?} is missing"))
    })
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

fn default_settings(product: &dyn ProductProfile) -> SettingsDraft {
    SettingsDraft {
        channels: product
            .channels()
            .iter()
            .map(|channel| ChannelSettingsDraft {
                id: gmofg_proxy_domain::ChannelId::new(channel.id)
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use gmofg_proxy_product_payment::PaymentProductProfile;
    use rusqlite::params;

    #[derive(Debug)]
    struct FixedLocalAddressProvider(Option<Ipv4Addr>);

    impl LocalAddressProvider for FixedLocalAddressProvider {
        fn preferred_lan_ipv4(&self) -> Option<Ipv4Addr> {
            self.0
        }
    }

    fn test_defaults() -> SettingsDraft {
        SettingsDraft {
            channels: vec![
                ChannelSettingsDraft {
                    id: gmofg_proxy_domain::ChannelId::new("alpha").unwrap(),
                    display_name: "Alpha".into(),
                    enabled: true,
                    port: 20_001,
                    upstream_url: "https://alpha.example.test".into(),
                },
                ChannelSettingsDraft {
                    id: gmofg_proxy_domain::ChannelId::new("beta").unwrap(),
                    display_name: "Beta".into(),
                    enabled: true,
                    port: 20_002,
                    upstream_url: "https://beta.example.test".into(),
                },
            ],
            ..SettingsDraft::default()
        }
    }

    fn adapter_with_address(address: Option<Ipv4Addr>) -> SettingsRepositoryAdapter {
        SettingsRepositoryAdapter::with_local_address_provider(
            Arc::new(SqliteStore::in_memory().expect("store")),
            test_defaults(),
            &[],
            Arc::new(FixedLocalAddressProvider(address)),
        )
    }

    fn valid_draft() -> SettingsDraft {
        test_defaults()
    }

    fn legacy_payment_settings_json() -> Value {
        serde_json::json!({
            "expected_revision": 2,
            "bind_address": "10.0.34.50",
            "transaction_enabled": true,
            "transaction_port": 26627,
            "dll_enabled": false,
            "dll_port": 26127,
            "upstream_transaction_url": "https://legacy-transaction.example.test",
            "upstream_dll_url": "https://legacy-dll.example.test",
            "connect_timeout_seconds": 11,
            "write_timeout_seconds": 12,
            "read_timeout_seconds": 13,
            "rewrite_host": false,
            "max_body_bytes": 1_048_576,
            "max_sessions": 99,
            "max_memory_bytes": 67_108_864,
            "leaf_sans": ["10.0.34.50"]
        })
    }

    fn create_legacy_settings_database(path: &std::path::Path, revision: u64, value: &Value) {
        let connection = rusqlite::Connection::open(path).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE settings (
                    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                    revision INTEGER NOT NULL,
                    json TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )
            .expect("legacy settings schema");
        connection
            .execute(
                "INSERT INTO settings(singleton_id, revision, json, updated_at)
                 VALUES (1, ?1, ?2, ?3)",
                params![
                    i64::try_from(revision).expect("test revision"),
                    value.to_string(),
                    Utc::now().to_rfc3339()
                ],
            )
            .expect("legacy settings row");
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
        let adapter = adapter_with_address(None);
        let saved = adapter.save(valid_draft()).await.expect("save");
        assert_eq!(saved.revision, 1);
        assert!(saved.effective.is_none());
        let applied = adapter
            .apply_effective(saved.stored.clone())
            .await
            .expect("apply");
        assert!(!applied.requires_restart);

        let mut changed = applied.stored;
        changed.channels[0].port = 20_003;
        let changed = adapter.save(changed).await.expect("change");
        assert!(changed.requires_restart);
        assert_eq!(
            changed.effective.expect("effective").channels[0].port,
            20_001
        );
    }

    #[tokio::test]
    async fn validation_reports_an_occupied_listener_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve port");
        let port = listener.local_addr().expect("local address").port();
        let adapter = adapter_with_address(None);
        let mut draft = valid_draft();
        draft.bind_address = "127.0.0.1".into();
        draft.channels[0].port = port;
        draft.leaf_sans = vec!["127.0.0.1".into()];
        let validation = adapter.validate(&draft).await.expect("validation");
        assert!(!validation.valid);
        assert!(validation.field_errors.contains_key("channels.alpha.port"));
    }

    #[tokio::test]
    async fn product_channel_catalog_rejects_unknown_missing_and_renamed_channels() {
        let adapter = adapter_with_address(None);
        let mut renamed = valid_draft();
        renamed.channels[0].display_name = "Spoofed".into();
        let validation = adapter.validate(&renamed).await.unwrap();
        assert!(
            validation
                .field_errors
                .contains_key("channels.alpha.display_name")
        );

        let mut unknown = valid_draft();
        unknown.channels.pop();
        unknown.channels.push(ChannelSettingsDraft {
            id: gmofg_proxy_domain::ChannelId::new("gamma").unwrap(),
            display_name: "Gamma".into(),
            enabled: false,
            port: 20_003,
            upstream_url: "https://gamma.example.test".into(),
        });
        let validation = adapter.validate(&unknown).await.unwrap();
        assert!(validation.field_errors.contains_key("channels"));
        assert!(validation.field_errors.contains_key("channels.gamma.id"));
    }

    #[tokio::test]
    async fn payment_profile_migrates_real_legacy_settings_sqlite_and_preserves_cas_revision() {
        let directory = tempfile::tempdir().expect("temp directory");
        let database = directory.path().join("legacy-payment.sqlite3");
        create_legacy_settings_database(&database, 7, &legacy_payment_settings_json());
        let store = Arc::new(SqliteStore::open(&database).expect("open legacy database"));
        let adapter =
            SettingsRepositoryAdapter::new(Arc::clone(&store), &PaymentProductProfile::default());

        let loaded = adapter.get().await.expect("load migrated settings");
        assert_eq!(loaded.revision, 7);
        assert_eq!(loaded.stored.expected_revision, Some(7));
        assert_eq!(
            loaded
                .stored
                .channels
                .iter()
                .map(|channel| (
                    channel.id.as_str(),
                    channel.display_name.as_str(),
                    channel.enabled,
                    channel.port,
                    channel.upstream_url.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "transaction",
                    "交易",
                    true,
                    26_627,
                    "https://legacy-transaction.example.test"
                ),
                (
                    "dll",
                    "DLL",
                    false,
                    26_127,
                    "https://legacy-dll.example.test"
                )
            ]
        );

        let saved = adapter
            .save(loaded.stored.clone())
            .await
            .expect("save migrated settings");
        assert_eq!(saved.revision, 8);
        let persisted = store.load_settings().unwrap().unwrap();
        assert_eq!(
            persisted
                .value
                .get(PERSISTENCE_VERSION_FIELD)
                .and_then(Value::as_u64),
            Some(SETTINGS_PERSISTENCE_VERSION)
        );
        assert!(persisted.value.get("transaction_enabled").is_none());

        let stale = adapter
            .save(loaded.stored)
            .await
            .expect_err("legacy revision must still participate in CAS");
        assert_eq!(stale.view_model.code, "REVISION_CONFLICT");
    }

    #[test]
    fn non_payment_profile_rejects_legacy_payment_settings_instead_of_consuming_them() {
        let error = deserialize_settings(legacy_payment_settings_json(), &test_defaults(), &[])
            .expect_err("non-Payment profile must reject Payment persistence");
        assert!(error.to_string().contains("missing field `channels`"));
    }
}
