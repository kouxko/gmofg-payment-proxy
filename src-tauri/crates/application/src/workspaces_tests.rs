use super::*;
use serde_json::Value;

#[tokio::test]
async fn default_store_contains_selected_safe_workspace() {
    let store = InMemoryWorkspaceStore::default();
    let list = store.list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].selected);
    let workspace = store.get(list[0].id).await.unwrap();
    assert!(workspace.validate().is_ok());
    assert_eq!(workspace.listeners.len(), 1);
}

#[tokio::test]
async fn create_validate_save_copy_select_and_delete_share_one_revision_contract() {
    let store = InMemoryWorkspaceStore::new_empty();
    let created = store.create("Lab".into()).await.unwrap();
    let validation = store.validate(created.clone()).await.unwrap();
    assert!(validation.valid);

    let mut edited = created.clone();
    edited.name = "Lab Updated".into();
    let saved = store.save(edited).await.unwrap();
    assert_eq!(saved.revision.get(), 2);
    assert!(store.save(created).await.is_err(), "stale save must fail");

    let copied = store.copy(saved.id).await.unwrap();
    assert_ne!(copied.id, saved.id);
    assert_eq!(copied.revision, intercept_proxy_domain::Revision::INITIAL);
    assert!(store.select(copied.id).await.unwrap().selected);
    assert_eq!(store.list().await.unwrap().len(), 2);

    store
        .delete(copied.id, copied.revision.get())
        .await
        .unwrap();
    assert!(store.get(copied.id).await.is_err());
    assert_eq!(store.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn export_round_trip_contains_references_but_no_secret_values() {
    let source = InMemoryWorkspaceStore::default();
    let id = source.list().await.unwrap()[0].id;
    let document = source.export_document(id).await.unwrap();
    let text = String::from_utf8(document.clone())
        .unwrap()
        .to_ascii_lowercase();
    for forbidden in ["password", "private_key", "pkcs12", "secret_value"] {
        assert!(!text.contains(forbidden), "{forbidden} leaked into export");
    }

    let destination = InMemoryWorkspaceStore::new_empty();
    let imported = destination.import_document(document).await.unwrap();
    assert!(imported.validate().is_ok());
    assert_eq!(destination.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn export_rejects_unmanaged_certificate_references() {
    let mut workspace = ProxyWorkspace::default();
    workspace
        .certificate_references
        .push(intercept_proxy_domain::CertificateReference {
            id: CertificateReferenceId::new(),
            label: "旧文件引用".into(),
            kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamServerTrust,
            reference: "file:/tmp/server-ca.pem".into(),
        });

    let error = serialize_workspace_document(&crate::WorkspaceDocument {
        format_version: crate::WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace,
        certificate_materials: Vec::new(),
    })
    .expect_err("portable export must reject unmanaged references");

    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
}

#[tokio::test]
async fn import_rejects_unknown_sensitive_fields_before_serde_discards_them() {
    for forbidden in ["password", "basic_auth_password", "pkcs12_password"] {
        let workspace = ProxyWorkspace::default();
        let mut value = serde_json::to_value(workspace).unwrap();
        value.as_object_mut().unwrap().insert(
            forbidden.into(),
            Value::String("must-not-enter-core".into()),
        );
        let document = serde_json::to_vec(&value).unwrap();
        let store = InMemoryWorkspaceStore::new_empty();
        let error = store.import_document(document).await.unwrap_err();
        assert_eq!(error.view_model.code, "IMPORT_FAILED");
        assert!(store.list().await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn import_rejects_unmanaged_certificate_references() {
    let mut workspace = ProxyWorkspace::default();
    workspace
        .certificate_references
        .push(intercept_proxy_domain::CertificateReference {
            id: CertificateReferenceId::new(),
            label: "外部证书".into(),
            kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamServerTrust,
            reference: "pkcs12:/tmp/client.p12?password_env=P12_PASSWORD".into(),
        });
    let store = InMemoryWorkspaceStore::new_empty();
    let document = serde_json::to_vec(&serde_json::json!({
        "format_version": crate::WORKSPACE_DOCUMENT_FORMAT_VERSION,
        "workspace": workspace,
        "certificate_materials": [],
    }))
    .unwrap();

    let error = store
        .import_document(document)
        .await
        .expect_err("unmanaged reference rejected");

    assert_eq!(
        error.view_model.code,
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED"
    );
    assert!(store.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn importing_same_document_twice_never_overwrites_existing_workspace() {
    let workspace = ProxyWorkspace::default();
    let document = serialize_workspace_document(&crate::WorkspaceDocument {
        format_version: crate::WORKSPACE_DOCUMENT_FORMAT_VERSION,
        workspace: workspace.clone(),
        certificate_materials: Vec::new(),
    })
    .unwrap();
    let store = InMemoryWorkspaceStore::new_empty();
    let first = store.import_document(document.clone()).await.unwrap();
    let second = store.import_document(document).await.unwrap();
    assert_ne!(first.id, workspace.id);
    assert_ne!(second.id, first.id);
    assert_eq!(store.list().await.unwrap().len(), 2);
}

#[test]
fn identity_remap_preserves_socket_target_and_remaps_all_tls_references() {
    use intercept_proxy_domain::{
        CertificateReference, CertificateReferenceKind, DownstreamClientAuthentication,
        ListenerDataPlane, SocketDownstreamTlsSettings, SocketEndpoint, SocketPayloadProcessing,
        SocketRelaySecurity, SocketRelaySettings, SocketUpstreamTlsSettings,
    };

    let server_identity = CertificateReferenceId::new();
    let client_trust = CertificateReferenceId::new();
    let server_trust = CertificateReferenceId::new();
    let client_identity = CertificateReferenceId::new();
    let old_ids = [server_identity, client_trust, server_trust, client_identity];
    let kinds = [
        CertificateReferenceKind::ReverseServerIdentity,
        CertificateReferenceKind::DownstreamClientTrust,
        CertificateReferenceKind::UpstreamServerTrust,
        CertificateReferenceKind::UpstreamClientIdentity,
    ];
    let mut workspace = ProxyWorkspace {
        certificate_references: old_ids
            .into_iter()
            .zip(kinds)
            .map(|(id, kind)| CertificateReference {
                id,
                label: id.to_string(),
                kind,
                reference: format!("managed:listener-tls:{id}"),
            })
            .collect(),
        ..ProxyWorkspace::default()
    };
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: "socket.example.test".into(),
            port: 16_127,
        },
        SocketRelaySecurity::TlsToTls {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity,
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: client_trust,
                },
            },
            upstream_tls: SocketUpstreamTlsSettings {
                verify_hostname: true,
                server_trust: Some(server_trust),
                client_identity: Some(client_identity),
            },
        },
        777,
        SocketPayloadProcessing::Direct,
    ));

    remap_workspace_identity(&mut workspace).unwrap();

    let ListenerDataPlane::Socket(settings) = &workspace.listeners[0].data_plane else {
        panic!("socket listener preserved")
    };
    let relay = settings.relay_topology().expect("Relay topology preserved");
    assert_eq!(relay.upstream.host, "socket.example.test");
    assert_eq!(relay.upstream.port, 16_127);
    assert_eq!(settings.maximum_connections, 777);
    let new_ids = workspace
        .certificate_references
        .iter()
        .map(|reference| reference.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(old_ids.into_iter().all(|id| !new_ids.contains(&id)));
    workspace.validate().unwrap();
}
