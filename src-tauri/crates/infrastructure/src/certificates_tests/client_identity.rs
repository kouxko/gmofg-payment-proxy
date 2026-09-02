#[test]
fn pem_client_identity_accepts_client_auth_chain_and_private_key() {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    let (identity, root) = signed_identity(
        CertificateService,
        "PEM Client",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
    );
    let pkcs12 = pfx(&[("client", &identity, &[&root])], "secret");
    let parsed_pkcs12 = CertificateService.parse_pkcs12(&pkcs12, "secret").unwrap();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(&parsed_pkcs12.certificate_der),
        STANDARD.encode(&parsed_pkcs12.chain_der[0]),
        STANDARD.encode(&parsed_pkcs12.private_key_pkcs8_der),
    );

    let parsed = CertificateService
        .parse_client_identity_pem(pem.as_bytes())
        .unwrap();

    assert_eq!(parsed.certificate_chain_der.len(), 2);
    assert_eq!(parsed.private_key_pkcs8_der, parsed_pkcs12.private_key_pkcs8_der);
}

#[test]
fn pem_client_identity_rejects_server_auth_identity() {
    let root = CertificateService.generate_root_ca("Server Root").unwrap();
    let leaf = CertificateService
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "server.test".into(),
                dns_names: vec!["server.test".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let pem = pem_server_identity(&[&leaf, &root], &leaf.private_key_pkcs8_der);

    let error = CertificateService
        .parse_client_identity_pem(&pem)
        .unwrap_err();

    assert!(error.to_string().contains("clientAuth EKU"));
}
