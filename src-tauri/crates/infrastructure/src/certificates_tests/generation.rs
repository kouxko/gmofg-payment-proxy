use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use p12_keystore::{
    Certificate, EncryptionAlgorithm, KeyStoreEntry, MacAlgorithm, PrivateKey, PrivateKeyChain,
};

#[test]
fn generates_root_and_leaf_with_expected_policies() {
    let service = CertificateService;
    let root = service
        .generate_root_ca("Intercept Proxy Test Root")
        .expect("root");
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "proxy.local".to_owned(),
                dns_names: vec!["proxy.local".to_owned()],
                ip_addresses: vec!["192.168.10.20".parse().expect("ip")],
            },
        )
        .expect("leaf");
    assert!(root.metadata.is_ca);
    assert!(!leaf.metadata.is_ca);
    assert_eq!(
        leaf.metadata.san,
        vec!["DNS:proxy.local", "IP:192.168.10.20"]
    );
    service
        .validate_leaf(
            &root.certificate_der,
            &leaf.certificate_der,
            &leaf.private_key_pkcs8_der,
            &["proxy.local".into(), "192.168.10.20".into()],
        )
        .expect("validate generated leaf");
}

#[test]
fn upstream_ca_bundle_preserves_every_valid_certificate_in_input_order() {
    let service = CertificateService;
    let intermediate = service.generate_root_ca("Bundle Intermediate").unwrap();
    let root = service.generate_root_ca("Bundle Root").unwrap();
    let bundle = pem_ca_bundle(&[&intermediate, &root]);

    let parsed = service.parse_upstream_ca(&bundle).unwrap();

    assert_eq!(
        parsed.certificate_chain_der,
        vec![
            intermediate.certificate_der.clone(),
            root.certificate_der.clone()
        ]
    );
    assert_eq!(parsed.certificate_der, root.certificate_der);
    let reparsed = service.parse_upstream_ca(parsed.canonical_bytes()).unwrap();
    assert_eq!(reparsed.certificate_chain_der, parsed.certificate_chain_der);
}

#[test]
fn upstream_ca_bundle_rejects_the_whole_input_when_any_certificate_is_not_a_ca() {
    let service = CertificateService;
    let root = service.generate_root_ca("Bundle Root").unwrap();
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "not-a-ca.example".into(),
                dns_names: vec!["not-a-ca.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();

    assert!(
        service
            .parse_upstream_ca(&pem_ca_bundle(&[&root, &leaf]))
            .is_err()
    );
}

#[test]
fn generated_leaf_allows_small_client_clock_skew() {
    let service = CertificateService;
    let root = service.generate_root_ca("Root").expect("root");
    let issued_at = ::time::OffsetDateTime::now_utc();
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "proxy.local".into(),
                dns_names: vec!["proxy.local".into()],
                ip_addresses: Vec::new(),
            },
        )
        .expect("leaf");
    let certificate = parse_der(&leaf.certificate_der).expect("parse leaf");
    let not_before =
        ::time::OffsetDateTime::from_unix_timestamp(certificate.validity().not_before.timestamp())
            .expect("valid timestamp");

    assert!(not_before <= issued_at - ::time::Duration::minutes(4));
    assert!(not_before >= issued_at - ::time::Duration::minutes(6));
}

#[test]
fn leaf_requires_san() {
    let service = CertificateService;
    let root = service.generate_root_ca("Root").expect("root");
    let result = service.generate_leaf(
        &root.certificate_der,
        &root.private_key_pkcs8_der,
        &LeafCertificateRequest {
            common_name: "proxy".to_owned(),
            dns_names: Vec::new(),
            ip_addresses: Vec::new(),
        },
    );
    assert!(matches!(
        result,
        Err(InfrastructureError::CertificateInvalid { .. })
    ));
}

#[test]
fn validation_rejects_wrong_key_chain_and_missing_san() {
    let service = CertificateService;
    let root = service.generate_root_ca("Root").expect("root");
    let other_root = service.generate_root_ca("Other Root").expect("other root");
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "proxy.local".into(),
                dns_names: vec!["proxy.local".into()],
                ip_addresses: Vec::new(),
            },
        )
        .expect("leaf");
    assert!(
        service
            .validate_leaf(
                &root.certificate_der,
                &leaf.certificate_der,
                &other_root.private_key_pkcs8_der,
                &["proxy.local".into()],
            )
            .is_err()
    );
    assert!(
        service
            .validate_leaf(
                &other_root.certificate_der,
                &leaf.certificate_der,
                &leaf.private_key_pkcs8_der,
                &["proxy.local".into()],
            )
            .is_err()
    );
    assert!(
        service
            .validate_leaf(
                &root.certificate_der,
                &leaf.certificate_der,
                &leaf.private_key_pkcs8_der,
                &["missing.local".into()],
            )
            .is_err()
    );
}

#[test]
fn ca_detection_uses_constraints_not_rendered_text() {
    let service = CertificateService;
    let root = service.generate_root_ca("Root").expect("root");
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "CA:TRUE".into(),
                dns_names: vec!["ca-true.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .expect("leaf");
    assert!(service.parse_ca(&leaf.certificate_der).is_err());
}

fn signed_identity(
    service: CertificateService,
    common_name: &str,
    key_usages: Vec<KeyUsagePurpose>,
    extended_key_usages: Vec<ExtendedKeyUsagePurpose>,
) -> (CertificateBundle, CertificateBundle) {
    let now = ::time::OffsetDateTime::now_utc();
    signed_identity_with_validity(
        service,
        common_name,
        key_usages,
        extended_key_usages,
        now,
        now + ::time::Duration::days(LEAF_VALIDITY_DAYS),
    )
}

fn signed_identity_with_validity(
    service: CertificateService,
    common_name: &str,
    key_usages: Vec<KeyUsagePurpose>,
    extended_key_usages: Vec<ExtendedKeyUsagePurpose>,
    not_before: ::time::OffsetDateTime,
    not_after: ::time::OffsetDateTime,
) -> (CertificateBundle, CertificateBundle) {
    let root = service
        .generate_root_ca(&format!("{common_name} Root"))
        .unwrap();
    let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &root.private_key_pkcs8_der.as_slice().into(),
        &PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let issuer =
        Issuer::from_ca_cert_der(&root.certificate_der.as_slice().into(), root_key).unwrap();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    params.distinguished_name = common_name_dn(common_name);
    params.not_before = not_before;
    params.not_after = not_after;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = key_usages;
    params.extended_key_usages = extended_key_usages;
    let certificate = params.signed_by(&key, &issuer).unwrap();
    (
        bundle(certificate.der().to_vec(), key.serialize_der()).unwrap(),
        root,
    )
}

fn server_chain_with_intermediate(
    intermediate_is_ca: bool,
    intermediate_not_before: ::time::OffsetDateTime,
    intermediate_not_after: ::time::OffsetDateTime,
) -> (CertificateBundle, CertificateBundle, CertificateBundle) {
    let service = CertificateService;
    let root = service.generate_root_ca("Server Chain Root").unwrap();
    let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &root.private_key_pkcs8_der.as_slice().into(),
        &PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let root_issuer =
        Issuer::from_ca_cert_der(&root.certificate_der.as_slice().into(), root_key).unwrap();

    let intermediate_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut intermediate_params = CertificateParams::default();
    intermediate_params.distinguished_name = common_name_dn("Server Chain Intermediate");
    intermediate_params.not_before = intermediate_not_before;
    intermediate_params.not_after = intermediate_not_after;
    if intermediate_is_ca {
        intermediate_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        intermediate_params.key_usages =
            vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    } else {
        intermediate_params.is_ca = IsCa::ExplicitNoCa;
        intermediate_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    }
    let intermediate_certificate = intermediate_params
        .signed_by(&intermediate_key, &root_issuer)
        .unwrap();
    let intermediate_issuer = Issuer::from_params(&intermediate_params, &intermediate_key);

    let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let now = ::time::OffsetDateTime::now_utc();
    let mut leaf_params = CertificateParams::default();
    leaf_params.distinguished_name = common_name_dn("Server Leaf");
    leaf_params.not_before = now - ::time::Duration::minutes(1);
    leaf_params.not_after = now + ::time::Duration::days(1);
    leaf_params.is_ca = IsCa::ExplicitNoCa;
    leaf_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let leaf_certificate = leaf_params
        .signed_by(&leaf_key, &intermediate_issuer)
        .unwrap();

    (
        bundle(leaf_certificate.der().to_vec(), leaf_key.serialize_der()).unwrap(),
        bundle(
            intermediate_certificate.der().to_vec(),
            intermediate_key.serialize_der(),
        )
        .unwrap(),
        root,
    )
}

fn pem_server_identity(
    certificate_chain: &[&CertificateBundle],
    private_key_pkcs8_der: &[u8],
) -> Vec<u8> {
    let mut pem = String::new();
    for certificate in certificate_chain {
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        pem.push_str(&STANDARD.encode(&certificate.certificate_der));
        pem.push_str("\n-----END CERTIFICATE-----\n");
    }
    pem.push_str("-----BEGIN PRIVATE KEY-----\n");
    pem.push_str(&STANDARD.encode(private_key_pkcs8_der));
    pem.push_str("\n-----END PRIVATE KEY-----\n");
    pem.into_bytes()
}

fn pem_ca_bundle(certificates: &[&CertificateBundle]) -> Vec<u8> {
    let mut pem = String::new();
    for certificate in certificates {
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        pem.push_str(&STANDARD.encode(&certificate.certificate_der));
        pem.push_str("\n-----END CERTIFICATE-----\n");
    }
    pem.into_bytes()
}
