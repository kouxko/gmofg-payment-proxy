use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose,
    IsCa, Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
};
use tokio::net::{TcpListener, TcpStream};

use intercept_proxy_runtime::{
    tls::{ClientTlsAdapter, ServerTlsAdapter},
    transport::{BoxIo, ConnectionAcceptor},
};

struct TlsTestIdentity {
    certificate: Vec<u8>,
    private_key: Vec<u8>,
    root: Vec<u8>,
}

fn tls_test_identity(common_name: &str, client: bool) -> TlsTestIdentity {
    let root_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut root_params = CertificateParams::default();
    root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let root = root_params.self_signed(&root_key).unwrap();
    let issuer = Issuer::from_ca_cert_der(root.der(), root_key).unwrap();
    let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut leaf_params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    leaf_params.distinguished_name = name;
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.extended_key_usages = vec![if client {
        ExtendedKeyUsagePurpose::ClientAuth
    } else {
        ExtendedKeyUsagePurpose::ServerAuth
    }];
    leaf_params.subject_alt_names = vec![SanType::DnsName("localhost".try_into().unwrap())];
    let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();
    TlsTestIdentity {
        certificate: leaf.der().to_vec(),
        private_key: leaf_key.serialize_der(),
        root: root.der().to_vec(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fresh_epoch_first_real_tls_handshake_reaches_rule_actor_and_rejects_in_verifier() {
    let mut rule = tls_fingerprint_reject_rule("unused");
    rule.draft.conditions.clear();
    let rules = Arc::new(StaticRules {
        snapshot: Mutex::new(RuleRuntimeSnapshot::new(vec![
            view_to_domain_rule(rule).unwrap(),
        ])),
    });
    let pipeline = Arc::new(RuntimePipelineAdapter::new(
        test_product_hooks(),
        rules.clone(),
        Arc::new(InMemorySessionStore::new(10, 64 * 1024 * 1024)),
        Arc::new(BreakpointCoordinator::default()),
        Arc::new(EventHub::new(128)),
        test_capture_repository(),
    ));
    let server = tls_test_identity("proxy.local", false);
    let client = tls_test_identity("client.local", true);
    let server_tls = ServerTlsAdapter::build(
        vec![server.certificate, server.root.clone()],
        server.private_key,
        client.root.clone(),
        None,
        pipeline.clone(),
    )
    .unwrap();
    let client_tls = ClientTlsAdapter::build(
        vec![client.certificate, client.root],
        client.private_key,
        server.root,
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let epoch = Uuid::new_v4();
    pipeline.runtime_started(epoch).await;
    let server_task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let context = test_context(epoch, Uuid::new_v4(), transaction_channel());
        server_tls.accept(Box::new(tcp) as BoxIo, &context).await
    });

    let tcp = TcpStream::connect(address).await.unwrap();
    assert!(client_tls.connect("localhost", Box::new(tcp)).await.is_err());
    assert!(server_task.await.unwrap().is_err());
    assert_eq!(rules.snapshot.lock().rules[0].hit_count, 1);
}
