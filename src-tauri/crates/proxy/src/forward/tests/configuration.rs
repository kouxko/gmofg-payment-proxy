#[test]
fn non_loopback_listener_fails_closed_without_both_controls() {
    let mut config = loopback_config();
    config.bind_addr = "0.0.0.0:8080".parse().unwrap();
    assert!(config.validate().is_err());
    config.authentication = ForwardAuthenticationMode::Required;
    assert!(config.validate().is_err());
    config.allowed_client_cidrs = vec!["10.0.0.0/8".into()];
    assert!(config.validate().is_ok());
}

#[tokio::test]
async fn listener_admission_can_project_plain_http_cidr_rejection() {
    let service = ForwardProxyService::new(loopback_config(), Arc::new(NoAuthentication)).unwrap();
    let (server, mut client) = tokio::io::duplex(512);

    crate::listener::ConnectionHandler::reject(
        &service,
        Box::new(server),
        service.connection_context("192.0.2.10:1234".parse().unwrap()),
        crate::listener::ListenerRejection::NetworkDenied,
        CancellationToken::new(),
    )
    .await;

    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.starts_with("HTTP/1.1 403 Forbidden\r\n"));
    assert!(response.ends_with("client address is not allowed"));
}

#[test]
fn mitm_allowlist_has_exact_and_domain_boundary_semantics() {
    let patterns = vec!["api.example.test".into(), "*.allowed.test".into()];
    assert!(authority_is_allowed("api.example.test", &patterns));
    assert!(authority_is_allowed("a.allowed.test", &patterns));
    assert!(authority_is_allowed("deep.a.allowed.test", &patterns));
    assert!(!authority_is_allowed("allowed.test", &patterns));
    assert!(!authority_is_allowed("badallowed.test", &patterns));
    assert!(!authority_is_allowed("api.example.test.evil", &patterns));
}
