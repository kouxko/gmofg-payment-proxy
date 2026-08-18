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

use intercept_proxy_application::{
    AppError, AppResult, ChannelSettingsDraft, FieldValidationViewModel, SettingsDraft,
    SettingsRepositoryPort, SettingsValidationViewModel, SettingsViewModel,
};
use intercept_proxy_domain::{
    CapacitySettings, ChannelSettings, Revision, Settings, TimeoutSettings, TlsVersion,
};
use intercept_proxy_product_api::ProductProfile;
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
    local_address: Arc<dyn LocalAddressProvider>,
}

impl SettingsRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>, product: &dyn ProductProfile) -> Self {
        Self::with_local_address_provider(
            store,
            default_settings(product),
            Arc::new(SystemLocalAddressProvider),
        )
    }

    fn with_local_address_provider(
        store: Arc<SqliteStore>,
        defaults: SettingsDraft,
        local_address: Arc<dyn LocalAddressProvider>,
    ) -> Self {
        Self {
            store,
            defaults,
            local_address,
        }
    }

    fn load_stored(&self) -> AppResult<(SettingsDraft, u64)> {
        let (mut draft, revision) = match infra(self.store.load_settings())? {
            Some(stored) => {
                let mut draft = deserialize_settings(stored.value)
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
        Ok(SettingsViewModel {
            stored,
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

    fn validate_ports(draft: &SettingsDraft, validation: &mut SettingsValidationViewModel) {
        let Ok(bind_address) = draft.bind_address.parse::<IpAddr>() else {
            return;
        };
        for channel in &draft.channels {
            if channel.enabled
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
        if self.defaults.channels.is_empty() {
            // 通用 Intercept Proxy 的监听器已经迁移到 Workspace。旧安装快照中的产品
            // 通道既不迁移也不继续运行，加载时直接丢弃，确保系统设置不会成为第二份
            // 入口配置来源。
            draft.channels.clear();
            return Ok(());
        }
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

mod mapping;
mod persistence;
mod port;

use mapping::{default_settings, to_domain_settings, valid};
use persistence::deserialize_settings;
pub(crate) use persistence::serialize_settings;

#[cfg(test)]
#[path = "settings/tests.rs"]
mod tests;
