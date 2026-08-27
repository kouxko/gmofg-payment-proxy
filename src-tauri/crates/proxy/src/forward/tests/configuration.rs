#[test]
fn non_loopback_listener_requires_authentication() {
    let mut config = loopback_config();
    config.bind_addr = "0.0.0.0:8080".parse().unwrap();
    assert!(config.validate().is_err());
    config.authentication = ForwardAuthenticationMode::Required;
    assert!(config.validate().is_ok());
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
