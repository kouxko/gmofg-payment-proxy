use super::*;
use intercept_proxy_domain::{
    DirectionProcessingOptions, ListenerDataPlane, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, ScriptedSocketProcessing, SocketDownstreamSecurity,
    SocketLocalResponderTopology, SocketPayloadProcessing, SocketRelaySettings, SocketTopology,
};

fn local_responder_listener(mut listener: ProxyListener) -> ProxyListener {
    listener.data_plane = ListenerDataPlane::Socket(SocketRelaySettings {
        topology: SocketTopology::LocalResponder(SocketLocalResponderTopology {
            downstream_security: SocketDownstreamSecurity::Tcp,
        }),
        maximum_connections: 32,
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
    listener
}

#[tokio::test]
async fn local_responder_real_facade_entries_fail_before_packages_certificates_or_runtime() {
    let ports = Arc::new(FakePorts::default());
    let application = application_with_fake_ports(ports);
    let mut workspace = application
        .workspace_create("Local responder".into())
        .await
        .unwrap();
    let listener = local_responder_listener(workspace.listeners[0].clone());

    let copy_error = application
        .listener_copy(listener.clone())
        .expect_err("T18 must not expose a LocalResponder creation path through copy");
    assert_eq!(copy_error.view_model.code, "LOCAL_RESPONDER_NOT_AVAILABLE");

    workspace.listeners[0] = listener.clone();
    let workspace = application.workspace_save(workspace).await.unwrap();
    let stale_revision = workspace.revision.get().saturating_sub(1);

    let stale_start = application
        .listener_start(workspace.id, stale_revision, listener.id)
        .await
        .expect_err("revision remains the first lifecycle gate");
    assert_eq!(stale_start.view_model.code, "REVISION_CONFLICT");

    let start_error = application
        .listener_start(workspace.id, workspace.revision.get(), listener.id)
        .await
        .expect_err("package lookup and runtime must not run for T18 LocalResponder");
    assert_eq!(start_error.view_model.code, "LOCAL_RESPONDER_NOT_AVAILABLE");

    let stale_connection = application
        .listener_test_upstream_connection(
            workspace.id,
            stale_revision,
            listener.clone(),
            Vec::new(),
        )
        .await
        .expect_err("connection probe keeps revision as its first gate");
    assert_eq!(stale_connection.view_model.code, "REVISION_CONFLICT");
    let stale_tls = application
        .listener_test_upstream_tls(workspace.id, stale_revision, listener.clone(), Vec::new())
        .await
        .expect_err("TLS probe keeps revision as its first gate");
    assert_eq!(stale_tls.view_model.code, "REVISION_CONFLICT");

    // 该引用故意来自不可信文件路径。如果 Facade 错误地进入证书校验，会先返回
    // LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED；稳定 unavailable 证明门禁顺序正确。
    let untrusted_reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "不应读取的证书".into(),
        kind: CertificateReferenceKind::ReverseServerIdentity,
        reference: "file:/tmp/must-not-be-read.pem".into(),
    };
    let connection_error = application
        .listener_test_upstream_connection(
            workspace.id,
            workspace.revision.get(),
            listener.clone(),
            vec![untrusted_reference.clone()],
        )
        .await
        .expect_err("LocalResponder has no upstream connection probe");
    assert_eq!(
        connection_error.view_model.code,
        "LOCAL_RESPONDER_NOT_AVAILABLE"
    );

    let tls_error = application
        .listener_test_upstream_tls(
            workspace.id,
            workspace.revision.get(),
            listener,
            vec![untrusted_reference],
        )
        .await
        .expect_err("LocalResponder has no upstream TLS probe");
    assert_eq!(tls_error.view_model.code, "LOCAL_RESPONDER_NOT_AVAILABLE");
}
