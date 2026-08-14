use crate::adapters::FileSelection;
use intercept_proxy_application::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, PortableSettings, SettingsDraft,
    WORKSPACE_DOCUMENT_FORMAT_VERSION, WorkspaceDocument,
};
use intercept_proxy_domain::{
    AndroidNetworkProfile, AndroidProxyRoute, AndroidTargetApplication, BodyCodecKind,
    CertificateReference, CertificateReferenceId, CertificateReferenceKind, ChannelId,
    ConnectionFaultAction, DownstreamClientAuthentication, DownstreamTlsSettings, FaultPreset,
    FaultPresetId, FixedServerSettings, HttpListenerSettings, ListenerDataPlane, ListenerId,
    MatchCondition, MessageStage, MetadataExtractor, MetadataExtractorId, MetadataExtractorSource,
    ProxyListener, ResponseAssertion, ResponseAssertionId, ResponseAssertionKind,
    Revision as DomainRevision, Rule, RuleId, UpstreamTlsSettings, WeakNetworkProfile,
};
use serde_json::Value;
use std::{collections::BTreeSet, path::PathBuf, sync::Mutex};

#[derive(Debug)]
struct RecordingDialog {
    open_path: Option<PathBuf>,
    save_selection: Option<FileSelection>,
    save_requests: Mutex<Vec<(String, String)>>,
}

impl RecordingDialog {
    fn opening(path: PathBuf) -> Self {
        Self {
            open_path: Some(path),
            save_selection: None,
            save_requests: Mutex::new(Vec::new()),
        }
    }

    fn saving(selection: Option<FileSelection>) -> Self {
        Self {
            open_path: None,
            save_selection: selection,
            save_requests: Mutex::new(Vec::new()),
        }
    }
}

impl NativeFileDialog for RecordingDialog {
    fn choose_open_file(&self, _: &str) -> AppResult<Option<PathBuf>> {
        Ok(self.open_path.clone())
    }

    fn choose_save_file(
        &self,
        purpose: &str,
        suggested_file_name: &str,
    ) -> AppResult<Option<FileSelection>> {
        self.save_requests
            .lock()
            .expect("save request lock")
            .push((purpose.into(), suggested_file_name.into()));
        Ok(self.save_selection.clone())
    }
}

#[tokio::test]
async fn workspace_document_dialog_cancellation_remains_a_normal_result() {
    let adapter = WorkspaceDocumentAdapter::new(Arc::new(RecordingDialog::saving(None)));

    assert_eq!(
        adapter.pick_import_document().await.expect("cancel import"),
        None
    );
    assert!(
        !adapter
            .save_export_document("workspace.intercept-workspace".into(), Vec::new())
            .await
            .expect("cancel export")
    );
}

#[tokio::test]
async fn workspace_document_import_uses_workspace_size_limit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("oversized.intercept-workspace");
    std::fs::File::create(&path)
        .expect("create oversized workspace")
        .set_len(MAX_WORKSPACE_DOCUMENT_BYTES as u64 + 1)
        .expect("set oversized workspace length");
    let adapter = WorkspaceDocumentAdapter::new(Arc::new(RecordingDialog::opening(path)));

    let error = adapter
        .pick_import_document()
        .await
        .expect_err("oversized workspace must be rejected");

    assert_eq!(error.view_model.code, "IMPORT_TOO_LARGE");
}

#[tokio::test]
async fn application_configuration_import_uses_configuration_size_limit() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("oversized.intercept-config");
    std::fs::File::create(&path)
        .expect("create oversized configuration")
        .set_len(MAX_APPLICATION_CONFIGURATION_BYTES as u64 + 1)
        .expect("set oversized configuration length");
    let adapter = WorkspaceDocumentAdapter::new(Arc::new(RecordingDialog::opening(path)));

    let error = adapter
        .pick_import_application_configuration()
        .await
        .expect_err("oversized configuration must be rejected");

    assert_eq!(error.view_model.code, "IMPORT_TOO_LARGE");
}

#[tokio::test]
async fn workspace_export_forwards_suggested_file_name_to_native_dialog() {
    let directory = tempfile::tempdir().expect("tempdir");
    let path = directory.path().join("chosen.intercept-workspace");
    let dialog = Arc::new(RecordingDialog::saving(Some(FileSelection {
        path: path.clone(),
        overwrite_confirmed: false,
    })));
    let adapter = WorkspaceDocumentAdapter::new(dialog.clone());

    assert!(
        adapter
            .save_export_document(
                "Lab_Updated.intercept-workspace".into(),
                b"workspace".to_vec(),
            )
            .await
            .expect("save workspace")
    );

    assert_eq!(std::fs::read(path).expect("read export"), b"workspace");
    assert_eq!(
        dialog
            .save_requests
            .lock()
            .expect("save request lock")
            .as_slice(),
        &[(
            "intercept_workspace".into(),
            "Lab_Updated.intercept-workspace".into()
        )]
    );
}

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

    let document = repository.export_document(saved.id).await.expect("export");
    let imported = repository
        .import_document(document)
        .await
        .expect("import copy");
    assert_ne!(imported.id, saved.id);
    assert_eq!(repository.list().await.expect("list after import").len(), 2);
}

#[tokio::test]
async fn workspace_import_rejects_secret_value_fields() {
    let repository = WorkspaceRepositoryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    ));
    let mut value = serde_json::to_value(ProxyWorkspace::default()).expect("workspace value");
    value["password"] = Value::String("must not persist".into());
    let error = repository
        .import_document(serde_json::to_vec(&value).expect("document"))
        .await
        .expect_err("secret field rejected");
    assert_eq!(error.view_model.code, "IMPORT_FAILED");
}

#[tokio::test]
async fn workspace_import_rejects_unmanaged_certificate_reference() {
    let repository = WorkspaceRepositoryAdapter::new(Arc::new(
        SqliteStore::in_memory().expect("in-memory store"),
    ));
    let mut workspace = ProxyWorkspace::default();
    workspace.certificate_references.push(CertificateReference {
        id: CertificateReferenceId::new(),
        label: "外部文件".into(),
        kind: CertificateReferenceKind::UpstreamServerTrust,
        reference: "file:/tmp/untrusted-ca.pem".into(),
    });

    let document = WorkspaceDocument {
        format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: Vec::new(),
        protocol_packages: Vec::new(),
    };
    let mut document = serde_json::to_value(document).expect("document value");
    // 该夹具模拟旧 v3 线格式；内部持久化新增字段不能混入可移植输入。
    document["workspace"]
        .as_object_mut()
        .expect("workspace object")
        .remove("socket_rules");
    document["workspace"]
        .as_object_mut()
        .expect("workspace object")
        .remove("socket_rule_created_order_high_water");
    let error = repository
        .import_document(serde_json::to_vec(&document).expect("document"))
        .await
        .expect_err("portable import must not read arbitrary local files");

    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
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
    assert_ne!(
        workspace.metadata_extractors[0].id,
        original.metadata_extractors[0].id
    );
    assert_ne!(
        workspace.response_assertions[0].id,
        original.response_assertions[0].id
    );
    assert_ne!(workspace.rules[0].id, original.rules[0].id);
    assert_ne!(workspace.fault_presets[0].id, original.fault_presets[0].id);
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
        metadata_extractors: vec![MetadataExtractor {
            id: MetadataExtractorId::new(),
            name: "Extractor".into(),
            listener_ids: vec![listener_id],
            source: MetadataExtractorSource::BodyText,
        }],
        response_assertions: vec![ResponseAssertion {
            id: ResponseAssertionId::new(),
            name: "Assertion".into(),
            listener_ids: vec![listener_id],
            enabled: true,
            assertion: ResponseAssertionKind::HttpStatusEquals { expected: 200 },
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
        socket_rules: Vec::new(),
        socket_rule_created_order_high_water: 0,
        fault_presets: vec![FaultPreset {
            id: FaultPresetId::new(),
            name: "Fault".into(),
            description: String::new(),
            connection_actions: vec![ConnectionFaultAction::Delay { milliseconds: 1 }],
            http_actions: Vec::new(),
        }],
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
