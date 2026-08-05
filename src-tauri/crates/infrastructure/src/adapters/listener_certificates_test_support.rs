use base64::{Engine as _, engine::general_purpose::STANDARD};
use p12_keystore::{Certificate, KeyStore, KeyStoreEntry, PrivateKey, PrivateKeyChain};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};

use crate::{CertificateService, LeafCertificateRequest};

pub(super) fn server_identity_pem() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let service = CertificateService;
    let root = service.generate_root_ca("Listener Server Root").unwrap();
    let leaf = service
        .generate_leaf(
            &root.certificate_der,
            &root.private_key_pkcs8_der,
            &LeafCertificateRequest {
                common_name: "listener.test".into(),
                dns_names: vec!["listener.test".into()],
                ip_addresses: Vec::new(),
            },
        )
        .unwrap();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
        STANDARD.encode(&leaf.certificate_der),
        STANDARD.encode(&root.certificate_der),
        STANDARD.encode(&leaf.private_key_pkcs8_der),
    )
    .into_bytes();
    (
        pem,
        leaf.private_key_pkcs8_der.to_vec(),
        root.certificate_der.clone(),
    )
}

pub(super) fn client_pkcs12() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let root = CertificateService
        .generate_root_ca("Listener Client Root")
        .unwrap();
    let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
        &root.private_key_pkcs8_der.as_slice().into(),
        &PKCS_ECDSA_P256_SHA256,
    )
    .unwrap();
    let issuer =
        Issuer::from_ca_cert_der(&root.certificate_der.as_slice().into(), root_key).unwrap();
    let client_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "Listener Client");
    params.distinguished_name = name;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let certificate = params.signed_by(&client_key, &issuer).unwrap();
    let private_key = client_key.serialize_der();
    let mut keystore = KeyStore::new();
    keystore.add_entry(
        "listener",
        KeyStoreEntry::PrivateKeyChain(PrivateKeyChain::new(
            "listener-key",
            PrivateKey::from_der(&private_key).unwrap(),
            [
                Certificate::from_der(certificate.der()).unwrap(),
                Certificate::from_der(&root.certificate_der).unwrap(),
            ],
        )),
    );
    (
        keystore.writer("password").write().unwrap(),
        private_key,
        root.certificate_der.clone(),
    )
}
