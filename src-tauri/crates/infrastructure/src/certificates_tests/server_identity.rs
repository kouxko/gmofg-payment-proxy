#[test]
fn pem_server_identity_accepts_valid_ordered_chain() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &intermediate, &root], &leaf.private_key_pkcs8_der);

    let parsed = CertificateService.parse_server_identity_pem(&pem).unwrap();

    assert_eq!(parsed.certificate_chain_der.len(), 3);
}

#[test]
fn pem_server_identity_rejects_broken_chain() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, _, _) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let unrelated_root = CertificateService
        .generate_root_ca("Unrelated Root")
        .unwrap();
    let pem = pem_server_identity(&[&leaf, &unrelated_root], &leaf.private_key_pkcs8_der);

    let error = CertificateService
        .parse_server_identity_pem(&pem)
        .unwrap_err();

    assert!(error.to_string().contains("签发关系或顺序无效"));
}

#[test]
fn pem_server_identity_rejects_reversed_chain_order() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &root, &intermediate], &leaf.private_key_pkcs8_der);

    let error = CertificateService
        .parse_server_identity_pem(&pem)
        .unwrap_err();

    assert!(error.to_string().contains("签发关系或顺序无效"));
}

#[test]
fn pem_server_identity_rejects_non_ca_intermediate() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        false,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &intermediate, &root], &leaf.private_key_pkcs8_der);

    let error = CertificateService
        .parse_server_identity_pem(&pem)
        .unwrap_err();

    assert!(error.to_string().contains("CA Basic Constraints"));
}

#[test]
fn pem_server_identity_rejects_expired_intermediate() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::days(2),
        now - ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &intermediate, &root], &leaf.private_key_pkcs8_der);

    let error = CertificateService
        .parse_server_identity_pem(&pem)
        .unwrap_err();

    assert!(error.to_string().contains("有效期"));
}

fn pfx(
    identities: &[(&str, &CertificateBundle, &[&CertificateBundle])],
    password: &str,
) -> Vec<u8> {
    let mut store = KeyStore::new();
    for (alias, identity, chain) in identities {
        let certificates = std::iter::once(*identity)
            .chain(chain.iter().copied())
            .map(|certificate| Certificate::from_der(&certificate.certificate_der).unwrap());
        store.add_entry(
            alias,
            KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
                format!("{alias}-key"),
                PrivateKey::from_der(&identity.private_key_pkcs8_der).unwrap(),
                certificates,
            )),
        );
    }
    store.writer(password).write().unwrap()
}

fn identity_with_legacy_self_signed_root(
    common_name: &str,
) -> (CertificateBundle, CertificateBundle) {
    let now = ::time::OffsetDateTime::now_utc();
    let root_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut root_params = CertificateParams::default();
    root_params.distinguished_name = common_name_dn(&format!("{common_name} Legacy Root"));
    root_params.not_before = now;
    root_params.not_after = now + ::time::Duration::days(ROOT_VALIDITY_DAYS);
    root_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    root_params.key_usages.clear();
    let root_certificate = root_params.self_signed(&root_key).unwrap();
    let issuer = Issuer::from_params(&root_params, &root_key);

    let client_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut client_params = CertificateParams::default();
    client_params.distinguished_name = common_name_dn(common_name);
    client_params.not_before = now;
    client_params.not_after = now + ::time::Duration::days(LEAF_VALIDITY_DAYS);
    client_params.is_ca = IsCa::ExplicitNoCa;
    client_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let client_certificate = client_params.signed_by(&client_key, &issuer).unwrap();

    (
        bundle(
            client_certificate.der().to_vec(),
            client_key.serialize_der(),
        )
        .unwrap(),
        bundle(root_certificate.der().to_vec(), root_key.serialize_der()).unwrap(),
    )
}
