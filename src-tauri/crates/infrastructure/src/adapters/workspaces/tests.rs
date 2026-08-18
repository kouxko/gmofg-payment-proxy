use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, PortableSettings, SettingsDraft,
};
use intercept_proxy_domain::{
    AndroidNetworkProfile, AndroidProxyRoute, AndroidTargetApplication, BodyCodecKind,
    CertificateReference, CertificateReferenceId, CertificateReferenceKind, ChannelId,
    DownstreamClientAuthentication, DownstreamTlsSettings, FixedServerSettings,
    HttpListenerSettings, ListenerDataPlane, ListenerId, MatchCondition, MessageStage,
    ProxyListener, Revision as DomainRevision, Rule, RuleId, UpstreamTlsSettings,
    WeakNetworkProfile,
};
use std::collections::BTreeSet;

#[tokio::test]
async fn sqlite_store_round_trips_and_rejects_stale_workspace_writes() {
    let repository = WorkspaceRepositoryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    ));
    let created = repository.create("Lab".into()).await.expect("create");
    assert!(repository.list().await.expect("list")[0].selected);

    let mut edited = created.clone();
    edited.name = "Updated".into();
    let saved = repository.save(edited).await.expect("save");
    assert_eq!(saved.revision, DomainRevision::new(2));
    assert!(repository.save(created).await.is_err());
}

#[tokio::test]
async fn full_configuration_replaces_workspaces_selection_and_settings_together() {
    let store = Arc::new(SqliteStore::in_memory().expect("in-memory store"));
    let repository = WorkspaceRepositoryAdapter::new(store.clone());
    let first = ProxyWorkspace {
        name: "First".into(),
        ..ProxyWorkspace::default()
    };
    let second = ProxyWorkspace {
        name: "Second".into(),
        ..ProxyWorkspace::default()
    };
    let settings = SettingsDraft {
        max_sessions: 777,
        ..SettingsDraft::default()
    };
    let document = ApplicationConfigurationDocument {
        format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
        selected_workspace_id: second.id,
        workspaces: vec![first.clone(), second.clone()],
        settings: PortableSettings::from(&settings),
        certificate_materials: Vec::new(),
        protocol_packages: Vec::new(),
    };

    repository
        .replace_all(document)
        .await
        .expect("atomic replace");

    let snapshot = store.load_workspaces().expect("workspaces");
    assert_eq!(snapshot.selected_id, Some(second.id.as_uuid()));
    assert_eq!(snapshot.records.len(), 2);
    let stored_settings = store.load_settings().expect("settings").expect("stored");
    assert_eq!(
        stored_settings.value["max_sessions"],
        serde_json::json!(777)
    );
}

#[test]
fn copy_identity_remaps_nested_ids_and_references() {
    let mut workspace = referenced_workspace();
    let original = workspace.clone();

    remap_workspace_identity(&mut workspace).expect("remap");

    assert_ne!(workspace.id, original.id);
    assert_eq!(workspace.revision, DomainRevision::INITIAL);
    assert_ne!(workspace.listeners[0].id, original.listeners[0].id);
    assert_ne!(workspace.rules[0].id, original.rules[0].id);
    assert_ne!(
        workspace.android_network_profiles[0].id,
        original.android_network_profiles[0].id
    );
    assert_ne!(
        workspace.certificate_references[0].id,
        original.certificate_references[0].id
    );

    let listener = &workspace.listeners[0];
    let http = listener.http().expect("migrated HTTP listener");
    assert_eq!(http.request_body_codec, BodyCodecKind::Utf8);
    assert_eq!(http.response_body_codec, BodyCodecKind::ShiftJis);
    assert_eq!(
        http.downstream_tls.server_identity,
        Some(workspace.certificate_references[0].id)
    );
    assert_eq!(
        workspace.rules[0].channel.as_ref().map(ChannelId::as_str),
        Some(listener.id.to_string().as_str())
    );
    assert_eq!(
        workspace.android_network_profiles[0].proxy_routes[0].listener_id,
        listener.id
    );
}

#[allow(clippy::too_many_lines)]
fn referenced_workspace() -> ProxyWorkspace {
    let listener_id = ListenerId::new();
    let server_identity = CertificateReferenceId::new();
    let downstream_trust = CertificateReferenceId::new();
    let upstream_identity = CertificateReferenceId::new();
    let upstream_trust = CertificateReferenceId::new();
    let certificates = [
        (
            server_identity,
            CertificateReferenceKind::ReverseServerIdentity,
        ),
        (
            downstream_trust,
            CertificateReferenceKind::DownstreamClientTrust,
        ),
        (
            upstream_identity,
            CertificateReferenceKind::UpstreamClientIdentity,
        ),
        (
            upstream_trust,
            CertificateReferenceKind::UpstreamServerTrust,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (id, kind))| CertificateReference {
        id,
        label: format!("certificate-{index}"),
        kind,
        reference: format!("pem:/tmp/certificate-{index}.pem"),
    })
    .collect();
    let workspace = ProxyWorkspace {
        id: WorkspaceId::new(),
        name: "Referenced".into(),
        revision: DomainRevision::new(9),
        listeners: vec![ProxyListener {
            id: listener_id,
            name: "Reverse".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: 18443,
            data_plane: ListenerDataPlane::Http(HttpListenerSettings {
                downstream_tls: DownstreamTlsSettings {
                    enabled: true,
                    server_identity: Some(server_identity),
                    dynamic_sni_allowlist: Vec::new(),
                    client_authentication: DownstreamClientAuthentication::Required {
                        trust: downstream_trust,
                    },
                },
                request_body_codec: BodyCodecKind::Utf8,
                response_body_codec: BodyCodecKind::ShiftJis,
                fixed_server: Some(FixedServerSettings {
                    upstream_url: "https://example.test".into(),
                    upstream_tls: UpstreamTlsSettings {
                        verify_hostname: true,
                        server_trust: Some(upstream_trust),
                        client_identity: Some(upstream_identity),
                    },
                }),
                ..HttpListenerSettings::default()
            }),
            ..ProxyListener::default()
        }],
        rules: vec![Rule {
            id: RuleId::new(),
            revision: DomainRevision::INITIAL,
            name: "Rule".into(),
            description: String::new(),
            enabled: true,
            priority: 1,
            created_order: 1,
            channel: Some(ChannelId::new(listener_id.to_string()).expect("channel")),
            stage: MessageStage::Request,
            conditions: Vec::<MatchCondition>::new(),
            actions: Vec::new(),
            one_shot: false,
            hit_count: 0,
            last_hit_at: None,
        }],
        protocol_rules: Vec::new(),
        protocol_rule_created_order_high_water: 0,
        certificate_references: certificates,
        android_network_profiles: vec![AndroidNetworkProfile {
            id: "android-profile".into(),
            name: "Android Profile".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.client".into(),
                uid: 10_001,
                display_name: None,
            }],
            destination_targets: Vec::new(),
            proxy_routes: vec![AndroidProxyRoute {
                destination: "example.test".into(),
                ports: vec![443],
                listener_id,
            }],
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        }],
    };
    workspace.validate().expect("valid referenced workspace");
    workspace
}
use super::*;

include!("tests/persistence_migration.rs");
