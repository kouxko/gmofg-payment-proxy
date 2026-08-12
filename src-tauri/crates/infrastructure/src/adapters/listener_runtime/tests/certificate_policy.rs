use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::ListenerRuntimePort;
use intercept_proxy_domain::{
    FixedServerSettings, HttpListenerSettings, ListenerDataPlane, ListenerId, ProxyListener,
    ProxyWorkspace, Revision, UpstreamTlsSettings,
};
use intercept_proxy_runtime::{
    ConnectionContext, ErrorCode, FaultAction, HandshakePolicy, Message, NoopPipelinePorts,
    PipelinePorts, ProxyError,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::*;
use crate::WorkspaceRecord;

#[derive(Debug)]
struct StaticInstallationIdentity;

impl InstallationServerIdentityProvider for StaticInstallationIdentity {
    fn load_installation_server_identity(&self) -> AppResult<ReverseClientIdentity> {
        Ok(ReverseClientIdentity {
            certificate_chain_der: vec![vec![1, 2, 3]],
            private_key_pkcs8_der: Zeroizing::new(vec![4, 5, 6]),
        })
    }
}

#[derive(Debug)]
struct StaticDynamicAuthority;

impl MitmCertificateAuthority for StaticDynamicAuthority {
    fn issue_server_identity(
        &self,
        _authority_host: &str,
    ) -> intercept_proxy_runtime::Result<intercept_proxy_runtime::MitmServerIdentity> {
        Err(ProxyError::new(
            ErrorCode::ConfigInvalid,
            "test authority does not issue certificates",
        ))
    }
}

#[test]
fn installation_root_enables_allowlisted_sni_dynamic_signing() {
    let listener = ProxyListener {
        data_plane: ListenerDataPlane::Http(HttpListenerSettings {
            downstream_tls: intercept_proxy_domain::DownstreamTlsSettings {
                enabled: true,
                server_identity: None,
                dynamic_sni_allowlist: vec!["api.example.test".into()],
                client_authentication: DownstreamClientAuthentication::Disabled,
            },
            ..HttpListenerSettings::default()
        }),
        ..ProxyListener::default()
    };
    let workspace = ProxyWorkspace {
        listeners: vec![listener.clone()],
        ..ProxyWorkspace::default()
    };
    let runtime = ListenerRuntimeAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()))
        .with_installation_server_identity(Arc::new(StaticInstallationIdentity))
        .with_mitm_certificate_authority(Arc::new(StaticDynamicAuthority));

    let tls = runtime
        .downstream_tls(
            &workspace,
            &listener,
            listener.http().expect("HTTP listener"),
        )
        .unwrap()
        .expect("downstream TLS");
    assert!(tls.dynamic_server_identity.is_some());
    assert_eq!(tls.dynamic_server_name_allowlist, vec!["api.example.test"]);
    assert_eq!(
        tls.server_identity.certificate_chain_der,
        vec![vec![1, 2, 3]]
    );
}

#[test]
fn unknown_issuer_explains_system_roots_without_weakening_explicit_trust() {
    let listener_id = ListenerId::new();
    let runtime_error = ProxyError::new(
        ErrorCode::TlsHandshakeFailed,
        "invalid peer certificate: UnknownIssuer",
    );

    let error = upstream_tls_test_error(listener_id, &runtime_error);

    assert_eq!(error.view_model.code, "TLS_HANDSHAKE_FAILED");
    assert_eq!(error.view_model.entity_id, Some(listener_id.to_string()));
    assert!(
        error
            .view_model
            .message
            .contains("不是该 Server 证书链的签发者")
    );
    assert_eq!(
        error.view_model.suggested_action.as_deref(),
        Some("公开 HTTPS 请选择“使用操作系统信任根”；私有 Server 请导入其真实签发 CA 后重试。")
    );
}

#[derive(Debug, Default)]
struct CountingPipeline {
    requests: AtomicUsize,
    responses: AtomicUsize,
}

#[test]
fn pem_identity_source_is_zeroizing_and_parse_errors_do_not_echo_secret_bytes() {
    let marker = "PRIVATE-MARKER-MUST-NOT-LEAK";
    let path = std::env::temp_dir().join(format!("intercept-identity-{}.pem", Uuid::new_v4()));
    fs::write(&path, format!("-----BEGIN PRIVATE KEY-----\n{marker}\n")).unwrap();

    let bytes = read_identity_reference_file(&path).unwrap();
    let _: &Zeroizing<Vec<u8>> = &bytes;
    assert!(String::from_utf8_lossy(&bytes).contains(marker));
    drop(bytes);

    let reference = CertificateReference {
        id: CertificateReferenceId::new(),
        label: "invalid identity".into(),
        kind: intercept_proxy_domain::CertificateReferenceKind::UpstreamClientIdentity,
        reference: format!("file:{}", path.display()),
    };
    let error = load_file_identity(&reference).unwrap_err();
    assert!(!error.view_model.message.contains(marker));
    let _ = fs::remove_file(path);
}

impl HandshakePolicy for CountingPipeline {}

#[async_trait]
impl PipelinePorts for CountingPipeline {
    async fn request(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> intercept_proxy_runtime::Result<Vec<FaultAction>> {
        self.requests.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn response(
        &self,
        _context: &ConnectionContext,
        _message: &mut Message,
    ) -> intercept_proxy_runtime::Result<Vec<FaultAction>> {
        self.responses.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}
