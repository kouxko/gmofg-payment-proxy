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
                id: intercept_proxy_domain::ChannelId::new("alpha").unwrap(),
                display_name: "Alpha".into(),
                enabled: true,
                port: 20_001,
                upstream_url: "https://alpha.example.test".into(),
            },
            ChannelSettingsDraft {
                id: intercept_proxy_domain::ChannelId::new("beta").unwrap(),
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
        id: intercept_proxy_domain::ChannelId::new("gamma").unwrap(),
        display_name: "Gamma".into(),
        enabled: false,
        port: 20_003,
        upstream_url: "https://gamma.example.test".into(),
    });
    let validation = adapter.validate(&unknown).await.unwrap();
    assert!(validation.field_errors.contains_key("channels"));
    assert!(validation.field_errors.contains_key("channels.gamma.id"));
}
use super::*;
