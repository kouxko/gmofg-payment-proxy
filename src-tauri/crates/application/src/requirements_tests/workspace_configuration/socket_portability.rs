use super::*;

#[tokio::test]
async fn socket_workspace_round_trip_restores_all_tls_roles_and_remaps_ids() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let roles = socket_certificate_roles();
    let original_ids = roles.each_ref().map(|reference| reference.id);
    workspace
        .certificate_references
        .extend(std::iter::once(installation_root()).chain(roles.iter().cloned()));
    configure_socket_listener(&mut workspace, &roles);
    workspaces.save(workspace).await.unwrap();
    let application =
        application_with_workspace_ports(ports, Arc::clone(&workspaces), Arc::clone(&documents));

    application.workspace_export(selected.id).await.unwrap();
    let (_, bytes) = documents.take_last_export().unwrap();
    let exported = parse_workspace_document(&bytes).unwrap();
    assert_eq!(exported.certificate_materials.len(), roles.len());
    assert_eq!(exported.workspace.certificate_references.len(), roles.len());
    assert_socket_materials(&exported, &roles);
    assert!(
        exported
            .certificate_materials
            .iter()
            .all(|material| material.kind != CertificateReferenceKind::MitmRootCa)
    );
    assert!(
        exported
            .workspace
            .certificate_references
            .iter()
            .all(|reference| { reference.reference != INSTALLATION_ROOT_CERTIFICATE_REFERENCE })
    );

    documents.set_next_import(bytes);
    application.workspace_import().await.unwrap();
    let imported_summary = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id != selected.id)
        .expect("imported workspace");
    let imported = workspaces.get(imported_summary.id).await.unwrap();
    assert_eq!(imported.certificate_references.len(), roles.len());
    assert!(imported.certificate_references.iter().all(|reference| {
        reference
            .reference
            .starts_with(MANAGED_LISTENER_CERTIFICATE_PREFIX)
            && reference.reference.contains("restored-")
            && !original_ids.contains(&reference.id)
    }));
    let imported_kinds = imported
        .certificate_references
        .iter()
        .map(|reference| reference.kind)
        .collect::<Vec<_>>();
    assert_eq!(imported_kinds.len(), 4);
    for kind in [
        CertificateReferenceKind::ReverseServerIdentity,
        CertificateReferenceKind::DownstreamClientTrust,
        CertificateReferenceKind::UpstreamServerTrust,
        CertificateReferenceKind::UpstreamClientIdentity,
    ] {
        assert!(imported_kinds.contains(&kind));
    }
    imported.validate().unwrap();
}

#[tokio::test]
async fn local_responder_tls_round_trip_remaps_only_app_side_certificates() {
    for required in [false, true] {
        local_responder_tls_round_trip(required).await;
    }
}

async fn local_responder_tls_round_trip(required: bool) {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let roles = socket_certificate_roles();
    let original_ids = [roles[0].id, roles[1].id];
    // 上游两种证书故意作为 orphan 加入。LocalResponder 导出必须只保留 App 侧身份与客户端 CA。
    workspace
        .certificate_references
        .extend(roles.iter().cloned());
    configure_local_responder_listener(&mut workspace, &roles, required);
    let original_processing = workspace.listeners[0].socket().unwrap().processing.clone();
    workspaces.save(workspace).await.unwrap();
    let application =
        application_with_workspace_ports(ports, Arc::clone(&workspaces), Arc::clone(&documents));

    application.workspace_export(selected.id).await.unwrap();
    let (_, bytes) = documents.take_last_export().unwrap();
    let exported = parse_workspace_document(&bytes).unwrap();
    assert_eq!(exported.certificate_materials.len(), 2);
    assert_eq!(exported.workspace.certificate_references.len(), 2);
    for role in &roles[..2] {
        assert!(
            exported
                .certificate_materials
                .iter()
                .any(|material| material.reference_id == role.id && material.kind == role.kind)
        );
    }
    assert!(exported.certificate_materials.iter().all(|material| {
        !matches!(
            material.kind,
            CertificateReferenceKind::UpstreamServerTrust
                | CertificateReferenceKind::UpstreamClientIdentity
        )
    }));

    documents.set_next_import(bytes);
    application.workspace_import().await.unwrap();
    let imported_summary = workspaces
        .list()
        .await
        .unwrap()
        .into_iter()
        .find(|summary| summary.id != selected.id)
        .expect("imported LocalResponder workspace");
    let imported = workspaces.get(imported_summary.id).await.unwrap();
    let imported_ids = imported
        .certificate_references
        .iter()
        .map(|reference| reference.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(imported_ids.len(), 2);
    assert!(
        original_ids
            .into_iter()
            .all(|id| !imported_ids.contains(&id))
    );

    let settings = imported.listeners[0].socket().unwrap();
    assert_eq!(settings.processing, original_processing);
    let intercept_proxy_domain::SocketTopology::LocalResponder(local) = &settings.topology else {
        panic!("LocalResponder topology must survive import")
    };
    let intercept_proxy_domain::SocketDownstreamSecurity::Tls { downstream_tls } =
        &local.downstream_security
    else {
        panic!("App-side TLS must survive import")
    };
    assert!(imported_ids.contains(&downstream_tls.server_identity));
    let trust = match (&downstream_tls.client_authentication, required) {
        (DownstreamClientAuthentication::Required { trust }, true)
        | (DownstreamClientAuthentication::Optional { trust }, false) => *trust,
        (DownstreamClientAuthentication::Optional { .. }, true) => {
            panic!("Required client authentication must not downgrade during import")
        }
        (DownstreamClientAuthentication::Required { .. }, false) => {
            panic!("Optional client authentication must not upgrade during import")
        }
        (DownstreamClientAuthentication::Disabled, _) => {
            panic!("client trust must survive import")
        }
    };
    assert!(imported_ids.contains(&trust));
    imported.validate().unwrap();
}

#[tokio::test]
async fn local_responder_tcp_export_drops_all_unreachable_certificates() {
    let ports = Arc::new(FakePorts::default());
    let workspaces = Arc::new(InMemoryWorkspaceStore::default());
    let documents = Arc::new(InMemoryWorkspaceDocumentStore::default());
    let selected = workspaces.list().await.unwrap().remove(0);
    let mut workspace = workspaces.get(selected.id).await.unwrap();
    let roles = socket_certificate_roles();
    workspace
        .certificate_references
        .extend(roles.iter().cloned());
    configure_local_responder_tcp_listener(&mut workspace);
    workspaces.save(workspace).await.unwrap();
    let application =
        application_with_workspace_ports(ports, Arc::clone(&workspaces), Arc::clone(&documents));

    application.workspace_export(selected.id).await.unwrap();
    let (_, bytes) = documents.take_last_export().unwrap();
    let exported = parse_workspace_document(&bytes).unwrap();
    assert!(exported.certificate_materials.is_empty());
    assert!(exported.workspace.certificate_references.is_empty());
}

fn socket_certificate_roles() -> [CertificateReference; 4] {
    [
        (
            "Socket 服务端身份",
            CertificateReferenceKind::ReverseServerIdentity,
        ),
        (
            "Socket 下游客户端 CA",
            CertificateReferenceKind::DownstreamClientTrust,
        ),
        (
            "Socket 上游 Server CA",
            CertificateReferenceKind::UpstreamServerTrust,
        ),
        (
            "Socket 上游客户端身份",
            CertificateReferenceKind::UpstreamClientIdentity,
        ),
    ]
    .map(|(label, kind)| CertificateReference {
        id: CertificateReferenceId::new(),
        label: label.into(),
        kind,
        reference: format!("{MANAGED_LISTENER_CERTIFICATE_PREFIX}{}", Uuid::new_v4()),
    })
}

fn installation_root() -> CertificateReference {
    CertificateReference {
        id: CertificateReferenceId::new(),
        label: "本机安装级 Root CA".into(),
        kind: CertificateReferenceKind::MitmRootCa,
        reference: INSTALLATION_ROOT_CERTIFICATE_REFERENCE.into(),
    }
}

fn configure_socket_listener(workspace: &mut ProxyWorkspace, roles: &[CertificateReference; 4]) {
    use intercept_proxy_domain::{
        ListenerDataPlane, SocketDownstreamTlsSettings, SocketEndpoint, SocketPayloadProcessing,
        SocketRelaySecurity, SocketRelaySettings, SocketUpstreamTlsSettings,
    };
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings::relay(
        SocketEndpoint {
            host: "socket.example.test".into(),
            port: 24_321,
        },
        SocketRelaySecurity::TlsToTls {
            downstream_tls: SocketDownstreamTlsSettings {
                server_identity: roles[0].id,
                client_authentication: DownstreamClientAuthentication::Required {
                    trust: roles[1].id,
                },
            },
            upstream_tls: SocketUpstreamTlsSettings {
                verify_hostname: true,
                server_trust: Some(roles[2].id),
                client_identity: Some(roles[3].id),
            },
        },
        321,
        SocketPayloadProcessing::Direct,
    ));
}

fn configure_local_responder_listener(
    workspace: &mut ProxyWorkspace,
    roles: &[CertificateReference; 4],
    required: bool,
) {
    use intercept_proxy_domain::{
        DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
        ProtocolPackageVersion, ScriptedSocketProcessing, SocketDownstreamSecurity,
        SocketDownstreamTlsSettings, SocketLocalResponderTopology, SocketPayloadProcessing,
        SocketRelaySettings, SocketTopology,
    };
    let client_authentication = if required {
        DownstreamClientAuthentication::Required { trust: roles[1].id }
    } else {
        DownstreamClientAuthentication::Optional { trust: roles[1].id }
    };
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tls {
                downstream_tls: SocketDownstreamTlsSettings {
                    server_identity: roles[0].id,
                    client_authentication,
                },
            },
        }),
        maximum_connections: 321,
        processing: SocketPayloadProcessing::Scripted(ScriptedSocketProcessing {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("iso8583-standard").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            upstream: DirectionProcessingOptions {
                decode_enabled: true,
                encode_enabled: false,
            },
            downstream: DirectionProcessingOptions {
                decode_enabled: false,
                encode_enabled: true,
            },
        }),
    });
}

fn configure_local_responder_tcp_listener(workspace: &mut ProxyWorkspace) {
    let roles = socket_certificate_roles();
    configure_local_responder_listener(workspace, &roles, false);
    let intercept_proxy_domain::ListenerDataPlane::Socket(settings) =
        &mut workspace.listeners[0].data_plane
    else {
        unreachable!()
    };
    let intercept_proxy_domain::SocketTopology::LocalResponder(local) = &mut settings.topology
    else {
        unreachable!()
    };
    local.downstream_security = intercept_proxy_domain::SocketDownstreamSecurity::Tcp;
}

fn assert_socket_materials(exported: &WorkspaceDocument, roles: &[CertificateReference; 4]) {
    for role in roles {
        let material = exported
            .certificate_materials
            .iter()
            .find(|material| material.reference_id == role.id)
            .expect("every Socket TLS role exported");
        assert_eq!(material.kind, role.kind);
        assert_eq!(
            material.password.is_some(),
            role.kind == CertificateReferenceKind::UpstreamClientIdentity
        );
    }
}
