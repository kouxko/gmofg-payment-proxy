#[test]
fn pem_server_identity_accepts_valid_ordered_chain() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &intermediate, &root], &leaf.private_key_pkcs8_der);

    let parsed = CertificateService
        .parse_server_identity_pem(&pem, "")
        .unwrap();

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
        .parse_server_identity_pem(&pem, "")
        .unwrap_err();

    assert!(error.to_string().contains("证书链不完整"));
}

#[test]
fn pem_server_identity_normalizes_unordered_chain() {
    let now = ::time::OffsetDateTime::now_utc();
    let (leaf, intermediate, root) = server_chain_with_intermediate(
        true,
        now - ::time::Duration::minutes(1),
        now + ::time::Duration::days(1),
    );
    let pem = pem_server_identity(&[&leaf, &root, &intermediate], &leaf.private_key_pkcs8_der);

    let parsed = CertificateService
        .parse_server_identity_pem(&pem, "")
        .unwrap();

    assert_eq!(parsed.certificate_chain_der[0], leaf.certificate_der);
    assert_eq!(parsed.certificate_chain_der[1], intermediate.certificate_der);
    assert_eq!(parsed.certificate_chain_der[2], root.certificate_der);
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
        .parse_server_identity_pem(&pem, "")
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
        .parse_server_identity_pem(&pem, "")
        .unwrap_err();

    assert!(error.to_string().contains("有效期"));
}

#[test]
fn server_pkcs12_uses_server_auth_policy_and_password_semantics() {
    let service = CertificateService;
    let root = service.generate_root_ca("Server P12 Root").unwrap();
    let server = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "server.example".into(),
                dns_names: vec!["server.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let protected = pfx(&[("server", &server, &[&root])], "correct");
    let parsed = service
        .parse_server_identity_pkcs12(&protected, "correct")
        .unwrap();
    assert_eq!(parsed.certificate_chain_der[0], server.certificate_der);
    assert!(matches!(
        service.parse_server_identity_pkcs12(&protected, "wrong"),
        Err(InfrastructureError::Pkcs12PasswordInvalid)
    ));

    let empty = pfx(&[("server", &server, &[&root])], "");
    assert!(service.parse_server_identity_pkcs12(&empty, "").is_ok());

    let legacy = pfx_legacy(&server, &root, "legacy");
    assert!(
        service
            .parse_server_identity_pkcs12(&legacy, "legacy")
            .is_ok()
    );

    let (client, client_root) = signed_identity(
        service,
        "Client only",
        vec![KeyUsagePurpose::DigitalSignature],
        vec![ExtendedKeyUsagePurpose::ClientAuth],
    );
    let client_p12 = pfx(&[("client", &client, &[&client_root])], "correct");
    assert!(
        service
            .parse_server_identity_pkcs12(&client_p12, "correct")
            .unwrap_err()
            .to_string()
            .contains("serverAuth")
    );
}

#[test]
fn pem_server_identity_rejects_missing_multiple_and_mismatched_keys() {
    let service = CertificateService;
    let root = service.generate_root_ca("Key Matrix Root").unwrap();
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "key-matrix.example".into(),
                dns_names: vec!["key-matrix.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let other = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "other.example".into(),
                dns_names: vec!["other.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let mut no_key = pem_server_identity(&[&leaf, &root], &leaf.private_key_pkcs8_der);
    let marker = no_key
        .windows(b"-----BEGIN PRIVATE KEY-----".len())
        .position(|window| window == b"-----BEGIN PRIVATE KEY-----")
        .unwrap();
    no_key.truncate(marker);
    assert!(
        service
            .parse_server_identity_pem(&no_key, "")
            .unwrap_err()
            .to_string()
            .contains("缺少私钥")
    );

    let mismatch = pem_server_identity(&[&leaf, &root], &other.private_key_pkcs8_der);
    assert!(
        service
            .parse_server_identity_pem(&mismatch, "")
            .unwrap_err()
            .to_string()
            .contains("匹配私钥")
    );

    let mut multiple = pem_server_identity(&[&leaf, &root], &leaf.private_key_pkcs8_der);
    multiple.extend_from_slice(
        pem_server_identity(&[], &other.private_key_pkcs8_der)
            .split_off(0)
            .as_slice(),
    );
    assert!(
        service
            .parse_server_identity_pem(&multiple, "")
            .unwrap_err()
            .to_string()
            .contains("只能包含一个私钥")
    );
}

#[test]
fn pem_server_identity_rejects_wrong_purpose_usage_and_validity() {
    let service = CertificateService;
    let now = ::time::OffsetDateTime::now_utc();
    let cases = [
        signed_identity(
            service,
            "Client purpose",
            vec![KeyUsagePurpose::DigitalSignature],
            vec![ExtendedKeyUsagePurpose::ClientAuth],
        ),
        signed_identity(
            service,
            "Missing signature",
            vec![KeyUsagePurpose::KeyEncipherment],
            vec![ExtendedKeyUsagePurpose::ServerAuth],
        ),
        signed_identity_with_validity(
            service,
            "Expired server",
            vec![KeyUsagePurpose::DigitalSignature],
            vec![ExtendedKeyUsagePurpose::ServerAuth],
            now - ::time::Duration::days(2),
            now - ::time::Duration::days(1),
        ),
    ];
    for (leaf, root) in cases {
        let pem = pem_server_identity(&[&leaf, &root], &leaf.private_key_pkcs8_der);
        assert!(service.parse_server_identity_pem(&pem, "").is_err());
    }
}

#[test]
fn pem_server_identity_accepts_encrypted_pkcs8_and_rejects_wrong_password() {
    use pkcs8::{DecodePrivateKey, PrivateKeyInfoOwned, der::pem::LineEnding};
    use std::convert::Infallible;

    #[derive(Debug)]
    struct TestRng(u64);
    impl pkcs8::rand_core::TryRng for TestRng {
        type Error = Infallible;

        fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            Ok((self.0 >> 32) as u32)
        }

        fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
            Ok(u64::from(self.try_next_u32()?) << 32 | u64::from(self.try_next_u32()?))
        }

        fn try_fill_bytes(&mut self, destination: &mut [u8]) -> Result<(), Self::Error> {
            for chunk in destination.chunks_mut(8) {
                let bytes = self.try_next_u64()?.to_ne_bytes();
                chunk.copy_from_slice(&bytes[..chunk.len()]);
            }
            Ok(())
        }
    }
    impl pkcs8::rand_core::TryCryptoRng for TestRng {}

    let service = CertificateService;
    let root = service.generate_root_ca("Encrypted Key Root").unwrap();
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "encrypted.example".into(),
                dns_names: vec!["encrypted.example".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let key = PrivateKeyInfoOwned::from_pkcs8_der(&leaf.private_key_pkcs8_der).unwrap();
    let encrypted = key
        .encrypt_with_rng(&mut TestRng(7), "pem-password")
        .unwrap()
        .to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)
        .unwrap();
    let mut pem = pem_server_identity(&[&root, &leaf], &leaf.private_key_pkcs8_der);
    let marker = pem
        .windows(b"-----BEGIN PRIVATE KEY-----".len())
        .position(|window| window == b"-----BEGIN PRIVATE KEY-----")
        .unwrap();
    pem.truncate(marker);
    pem.extend_from_slice(encrypted.as_bytes());

    let parsed = service
        .parse_server_identity_pem(&pem, "pem-password")
        .unwrap();
    assert_eq!(parsed.certificate_chain_der[0], leaf.certificate_der);
    assert!(
        service
            .parse_server_identity_pem(&pem, "wrong")
            .unwrap_err()
            .to_string()
            .contains("密码错误")
    );

    let encrypted_empty = key
        .encrypt_with_rng(&mut TestRng(11), "")
        .unwrap()
        .to_pem("ENCRYPTED PRIVATE KEY", LineEnding::LF)
        .unwrap();
    pem.truncate(marker);
    pem.extend_from_slice(encrypted_empty.as_bytes());
    assert!(service.parse_server_identity_pem(&pem, "").is_ok());
}

#[test]
fn pem_server_identity_accepts_pkcs1_and_normalizes_to_pkcs8() {
    let service = CertificateService;
    let mut pem = include_bytes!("fixtures/r09-root-cert.pem").to_vec();
    pem.extend_from_slice(include_bytes!("fixtures/r09-leaf-cert.pem"));
    pem.extend_from_slice(include_bytes!("fixtures/r09-leaf-key-pkcs1.pem"));

    let parsed = service.parse_server_identity_pem(&pem, "").unwrap();

    assert_eq!(parsed.certificate_chain_der.len(), 2);
    assert_eq!(parsed.metadata.subject, "CN=r09-pkcs1.example");
    assert!(pkcs8::PrivateKeyInfoRef::try_from(parsed.private_key_pkcs8_der.as_slice()).is_ok());
}

fn pfx_legacy(
    identity: &CertificateBundle,
    root: &CertificateBundle,
    password: &str,
) -> Vec<u8> {
    let mut store = KeyStore::new();
    store.add_entry(
        "legacy-server",
        KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
            "legacy-server-key",
            PrivateKey::from_der(&identity.private_key_pkcs8_der).unwrap(),
            [
                Certificate::from_der(&identity.certificate_der).unwrap(),
                Certificate::from_der(&root.certificate_der).unwrap(),
            ],
        )),
    );
    store
        .writer(password)
        .encryption_algorithm(EncryptionAlgorithm::PbeWithShaAnd3KeyTripleDesCbc)
        .mac_algorithm(MacAlgorithm::HmacSha1)
        .write()
        .unwrap()
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
