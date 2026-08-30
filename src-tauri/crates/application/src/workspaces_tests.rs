use super::*;

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

#[test]
fn unified_rule_remap_changes_rule_identity_preserves_revision_and_order_and_rebinds_listener() {
    use intercept_proxy_domain::{
        DocumentAction, ProtocolDirection, ProtocolDocumentRuleDefinition, ProtocolDocumentRuleId,
        ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion, ScriptedSocketProcessing,
        SocketEndpoint, SocketPayloadProcessing, SocketRelaySettings,
    };

    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("iso-8583").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let mut workspace = ProxyWorkspace::default();
    let old_listener_id = workspace.listeners[0].id;
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: "127.0.0.1".into(),
            port: 9000,
        },
        SocketRelaySecurity::Transparent,
        8,
        SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: package.clone(),
        }),
    ));
    let rule_id = ProtocolDocumentRuleId::new();
    let mut rule = ProtocolDocumentRuleDefinition::new(
        rule_id,
        true,
        -7,
        42,
        old_listener_id,
        package,
        ProtocolDirection::Upstream,
        vec![intercept_proxy_domain::DocumentCondition::Equals {
            field: intercept_proxy_domain::JsonPointer::property("trace_id"),
            value: intercept_proxy_domain::DocumentValue::String("phase5".into()),
        }],
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    rule.toggle(rule.revision(), false).unwrap();
    let original_revision = rule.revision();
    workspace.rule_created_order_high_water = rule.created_order();
    workspace
        .replace_document_runtime_rules(vec![rule])
        .unwrap();

    remap_workspace_identity(&mut workspace).unwrap();

    let remapped_rules = workspace.document_runtime_rules().unwrap();
    let remapped = &remapped_rules[0];
    assert_ne!(remapped.rule_id(), rule_id);
    assert_eq!(remapped.revision(), original_revision);
    assert_eq!(remapped.created_order(), 42);
    assert_eq!(remapped.priority(), -7);
    assert_ne!(workspace.listeners[0].id, old_listener_id);
    assert_eq!(remapped.listener_id(), workspace.listeners[0].id);
    workspace.validate().unwrap();
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
                tls_server_name: None,
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
