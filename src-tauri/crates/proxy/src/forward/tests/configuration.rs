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

#[test]
fn cidr_matching_is_family_safe() {
    let network = Network::parse("10.20.0.0/16").unwrap();
    assert!(network.contains("10.20.4.5".parse().unwrap()));
    assert!(network.contains("::ffff:10.20.4.5".parse().unwrap()));
    assert!(!network.contains("10.21.4.5".parse().unwrap()));
    assert!(!network.contains("::1".parse().unwrap()));
}

#[test]
fn adb_reverse_mapped_loopback_bypasses_only_the_cidr_gate() {
    let mut config = loopback_config();
    config.allowed_client_cidrs = vec!["10.20.0.0/16".into()];

    assert!(config.permits_peer("127.0.0.1".parse().unwrap()));
    assert!(config.permits_peer("::ffff:127.0.0.1".parse().unwrap()));
    assert!(config.permits_peer("10.20.4.5".parse().unwrap()));
    assert!(!config.permits_peer("10.21.4.5".parse().unwrap()));
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

#[tokio::test]
async fn mitm_leaf_cache_reuses_and_lru_evicts_with_configured_bound() {
    let certificate_authority = Arc::new(CountingCertificateAuthority::default());
    let runtime = ForwardMitmRuntime {
        config: ForwardMitmConfig {
            authority_allowlist: vec!["*.example.test".into()],
            maximum_cached_leaf_certificates: 1,
        },
        certificate_authority: certificate_authority.clone(),
        upstream_connector: Arc::new(NeverMitmUpstreamConnector),
        leaf_cache: Mutex::new(MitmLeafCache::new(1)),
    };
    runtime.server_config_for("one.example.test").await.unwrap();
    runtime.server_config_for("one.example.test").await.unwrap();
    assert_eq!(certificate_authority.count(), 1, "cache must reuse leaf");
    runtime.server_config_for("two.example.test").await.unwrap();
    runtime.server_config_for("one.example.test").await.unwrap();
    assert_eq!(
        certificate_authority.count(),
        3,
        "capacity one must evict the least-recently-used leaf"
    );
    assert_eq!(runtime.leaf_cache.lock().await.entries.len(), 1);
}
