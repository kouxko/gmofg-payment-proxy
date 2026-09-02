#[test]
fn pkcs12_requires_correct_password_and_exactly_one_identity() {
    let service = CertificateService;
    let (first, first_root) = signed_identity(
        service,
        "First",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
    );
    let (second, second_root) = signed_identity(
        service,
        "Second",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
    );
    let single = pfx(&[("first", &first, &[&first_root])], "secret");
    let parsed = service.parse_pkcs12(&single, "secret").unwrap();
    assert_eq!(parsed.certificate_der, first.certificate_der);
    assert!(matches!(
        service.parse_pkcs12(&single, "wrong"),
        Err(InfrastructureError::Pkcs12PasswordInvalid)
    ));
    let multiple = pfx(
        &[
            ("first", &first, &[&first_root]),
            ("second", &second, &[&second_root]),
        ],
        "secret",
    );
    let error = service.parse_pkcs12(&multiple, "secret").unwrap_err();
    assert!(error.to_string().contains("只能包含一个私钥身份"));
}

#[test]
fn pkcs12_supports_an_explicit_empty_password() {
    let service = CertificateService;
    let (identity, root) = signed_identity(
        service,
        "Empty password client",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
    );
    let empty_password = pfx(&[("client", &identity, &[&root])], "");

    let parsed = service.parse_pkcs12(&empty_password, "").unwrap();

    assert_eq!(parsed.certificate_der, identity.certificate_der);
}

#[test]
fn pkcs12_accepts_only_final_self_signed_legacy_root_without_key_usage() {
    let service = CertificateService;
    let (identity, legacy_root) = identity_with_legacy_self_signed_root("Legacy Client");
    let legacy = pfx(&[("legacy", &identity, &[&legacy_root])], "secret");
    let parsed = service.parse_pkcs12(&legacy, "secret").unwrap();
    assert_eq!(parsed.certificate_der, identity.certificate_der);
    assert_eq!(parsed.chain_der, vec![legacy_root.certificate_der.clone()]);

    let (_, unrelated_legacy_root) = identity_with_legacy_self_signed_root("Unrelated Client");
    let invalid = pfx(
        &[("legacy", &identity, &[&unrelated_legacy_root])],
        "secret",
    );
    assert!(service.parse_pkcs12(&invalid, "secret").is_err());
}

#[test]
fn downstream_client_trust_accepts_legacy_root_without_key_usage() {
    let service = CertificateService;
    let (_, legacy_root) = identity_with_legacy_self_signed_root("Legacy Downstream Client");
    let mut explicit_trust_anchor = legacy_root.certificate_der.clone();
    let final_byte = explicit_trust_anchor
        .last_mut()
        .expect("certificate has signature bytes");
    *final_byte ^= 1;

    let parsed = service
        .parse_client_trust_anchor(&explicit_trust_anchor)
        .expect("legacy self-signed client trust anchor");

    assert_eq!(parsed.certificate_der, explicit_trust_anchor);
    assert!(service.parse_upstream_ca(&parsed.certificate_der).is_err());
}

#[test]
fn pkcs12_rejects_ca_server_auth_and_missing_client_usages() {
    let service = CertificateService;

    let ca = service.generate_root_ca("CA identity").unwrap();
    let ca_pfx = pfx(&[("ca", &ca, &[])], "secret");
    assert!(
        service
            .parse_pkcs12(&ca_pfx, "secret")
            .unwrap_err()
            .to_string()
            .contains("非 CA")
    );

    let root = service.generate_root_ca("Server Root").unwrap();
    let server = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "Server only".into(),
                dns_names: vec!["server.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let server_pfx = pfx(&[("server", &server, &[&root])], "secret");
    assert!(
        service
            .parse_pkcs12(&server_pfx, "secret")
            .unwrap_err()
            .to_string()
            .contains("clientAuth")
    );

    let (missing_usage, missing_root) =
        signed_identity(service, "Missing usages", Vec::new(), Vec::new());
    let missing_pfx = pfx(&[("missing", &missing_usage, &[&missing_root])], "secret");
    assert!(
        service
            .parse_pkcs12(&missing_pfx, "secret")
            .unwrap_err()
            .to_string()
            .contains("DigitalSignature")
    );
}

#[test]
fn pkcs12_rejects_expired_and_not_yet_valid_client_certificates() {
    let service = CertificateService;
    let now = ::time::OffsetDateTime::now_utc();
    let (expired, expired_root) = signed_identity_with_validity(
        service,
        "Expired",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
        now - ::time::Duration::days(2),
        now - ::time::Duration::days(1),
    );
    let expired_pfx = pfx(&[("expired", &expired, &[&expired_root])], "secret");
    assert!(
        service
            .parse_pkcs12(&expired_pfx, "secret")
            .unwrap_err()
            .to_string()
            .contains("有效期")
    );

    let (future, future_root) = signed_identity_with_validity(
        service,
        "Future",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
        now + ::time::Duration::days(1),
        now + ::time::Duration::days(2),
    );
    let future_pfx = pfx(&[("future", &future, &[&future_root])], "secret");
    assert!(
        service
            .parse_pkcs12(&future_pfx, "secret")
            .unwrap_err()
            .to_string()
            .contains("有效期")
    );
}
