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
        ListenerDataPlane, SocketDownstreamTlsSettings, SocketEndpoint, SocketRelaySecurity,
        SocketRelaySettings, SocketUpstreamTlsSettings,
    };
    workspace.listeners[0].data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        upstream: SocketEndpoint {
            host: "socket.example.test".into(),
            port: 24_321,
        },
        security: SocketRelaySecurity::TlsToTls {
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
        maximum_connections: 321,
    });
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
