use base64::{Engine as _, engine::general_purpose::STANDARD};
use pkcs8::{
    EncodePrivateKey, EncryptedPrivateKeyInfoRef, PrivateKeyInfoRef,
    der::asn1::{AnyRef, OctetStringRef},
    spki::AlgorithmIdentifierRef,
};
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs1KeyDer, PrivatePkcs8KeyDer};
use x509_parser::pem::Pem;
use zeroize::{Zeroize, Zeroizing};

use super::{
    CertificateMetadata, InfrastructureError, classify_pkcs12_error, invalid, metadata, parse_der,
    validate_ca, validate_key_match, validate_server_end_entity,
};

const RSA_ENCRYPTION_OID: pkcs8::ObjectIdentifier =
    pkcs8::ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");

/// A downstream TLS identity normalized to leaf-to-root DER plus PKCS#8.
pub struct ParsedServerIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub metadata: CertificateMetadata,
}

impl std::fmt::Debug for ParsedServerIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedServerIdentity")
            .field("certificate_chain_len", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl Drop for ParsedServerIdentity {
    fn drop(&mut self) {
        for certificate in &mut self.certificate_chain_der {
            certificate.zeroize();
        }
    }
}

impl ParsedServerIdentity {
    pub fn canonical_pem(&self) -> Zeroizing<Vec<u8>> {
        let mut pem = Zeroizing::new(Vec::new());
        for certificate in &self.certificate_chain_der {
            append_pem(&mut pem, "CERTIFICATE", certificate);
        }
        append_pem(&mut pem, "PRIVATE KEY", &self.private_key_pkcs8_der);
        pem
    }
}

pub(super) fn parse_server_pem(
    bytes: &[u8],
    password: &str,
) -> Result<ParsedServerIdentity, InfrastructureError> {
    let entries = Pem::iter_from_buffer(bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid("PEM 服务端身份格式无效"))?;
    let certificates = entries
        .iter()
        .filter(|entry| entry.label == "CERTIFICATE")
        .map(|entry| entry.contents.clone())
        .collect::<Vec<_>>();
    let mut keys = entries.iter().filter(|entry| {
        matches!(
            entry.label.as_str(),
            "PRIVATE KEY" | "ENCRYPTED PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY"
        )
    });
    let key = keys
        .next()
        .ok_or_else(|| invalid("PEM 服务端身份缺少私钥"))?;
    if keys.next().is_some() {
        return Err(invalid("PEM 服务端身份必须且只能包含一个私钥"));
    }
    let private_key = normalize_pem_key(&key.label, &key.contents, password)?;
    normalize_server_identity(certificates, private_key)
}

pub(super) fn normalize_server_identity(
    certificates: Vec<Vec<u8>>,
    private_key_pkcs8_der: Zeroizing<Vec<u8>>,
) -> Result<ParsedServerIdentity, InfrastructureError> {
    if certificates.is_empty() {
        return Err(invalid("服务端身份缺少证书链"));
    }
    let leaf_candidates = certificates
        .iter()
        .enumerate()
        .filter_map(|(index, certificate)| {
            validate_key_match(certificate, &private_key_pkcs8_der)
                .is_ok()
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if leaf_candidates.len() != 1 {
        return Err(invalid(format!(
            "服务端身份必须且只能有一张证书匹配私钥，实际为 {}",
            leaf_candidates.len()
        )));
    }

    let mut remaining = certificates;
    let leaf = remaining.remove(leaf_candidates[0]);
    validate_server_end_entity(&leaf, &private_key_pkcs8_der)?;
    let mut chain = vec![leaf];
    loop {
        let current = parse_der(chain.last().expect("chain has leaf"))?;
        if current.subject() == current.issuer() {
            if current.verify_signature(None).is_err() {
                return Err(invalid("服务端身份根证书自签名无效"));
            }
            break;
        }
        let candidates = remaining
            .iter()
            .enumerate()
            .filter_map(|(index, issuer_der)| {
                let issuer = parse_der(issuer_der).ok()?;
                (current.issuer() == issuer.subject()
                    && current.verify_signature(Some(issuer.public_key())).is_ok())
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if candidates.len() != 1 {
            return Err(invalid(if candidates.is_empty() {
                "服务端身份证书链不完整"
            } else {
                "服务端身份证书链存在多个可用签发者"
            }));
        }
        let issuer = remaining.remove(candidates[0]);
        validate_ca(&parse_der(&issuer)?)?;
        chain.push(issuer);
    }
    if !remaining.is_empty() {
        return Err(invalid("服务端身份包含不属于该证书链的额外证书"));
    }
    let metadata = metadata(&parse_der(&chain[0])?)?;
    Ok(ParsedServerIdentity {
        certificate_chain_der: chain,
        private_key_pkcs8_der,
        metadata,
    })
}

fn normalize_pem_key(
    label: &str,
    bytes: &[u8],
    password: &str,
) -> Result<Zeroizing<Vec<u8>>, InfrastructureError> {
    let normalized = match label {
        "PRIVATE KEY" => Zeroizing::new(bytes.to_vec()),
        "ENCRYPTED PRIVATE KEY" => EncryptedPrivateKeyInfoRef::try_from(bytes)
            .and_then(|key| key.decrypt(password.as_bytes()))
            .map(|document| document.to_bytes())
            .map_err(|_| invalid("PEM 服务端身份密码错误或加密私钥无效"))?,
        "RSA PRIVATE KEY" => wrap_pkcs1(bytes)?,
        "EC PRIVATE KEY" => {
            let key = PrivateKeyDer::Sec1(bytes.to_vec().into());
            let pair = rcgen::KeyPair::try_from(&key)
                .map_err(|_| invalid("PEM 服务端身份包含不支持的 EC 私钥"))?;
            Zeroizing::new(pair.serialize_der())
        }
        _ => return Err(invalid("PEM 服务端身份私钥格式不受支持")),
    };
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(normalized.as_slice()));
    rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|_| invalid("PEM 服务端身份包含不支持或无效的私钥"))?;
    Ok(normalized)
}

fn wrap_pkcs1(bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, InfrastructureError> {
    let key = PrivateKeyDer::Pkcs1(PrivatePkcs1KeyDer::from(bytes));
    rustls::crypto::ring::sign::any_supported_type(&key)
        .map_err(|_| invalid("PEM 服务端身份包含无效的 PKCS#1 私钥"))?;
    let private_key =
        OctetStringRef::new(bytes).map_err(|_| invalid("PKCS#1 私钥长度超出支持范围"))?;
    let info = PrivateKeyInfoRef::new(
        AlgorithmIdentifierRef {
            oid: RSA_ENCRYPTION_OID,
            parameters: Some(AnyRef::NULL),
        },
        private_key,
    );
    info.to_pkcs8_der()
        .map(|document| document.to_bytes())
        .map_err(|_| invalid("PKCS#1 私钥无法转换为 PKCS#8"))
}

fn append_pem(output: &mut Vec<u8>, label: &str, der: &[u8]) {
    output.extend_from_slice(format!("-----BEGIN {label}-----\n").as_bytes());
    let encoded = STANDARD.encode(der);
    for line in encoded.as_bytes().chunks(64) {
        output.extend_from_slice(line);
        output.push(b'\n');
    }
    output.extend_from_slice(format!("-----END {label}-----\n").as_bytes());
}

pub(super) fn parse_server_pkcs12(
    bytes: &[u8],
    password: &str,
) -> Result<ParsedServerIdentity, InfrastructureError> {
    let store = p12_keystore::KeyStore::from_pkcs12(
        bytes,
        password,
        p12_keystore::Pkcs12ImportPolicy::Strict,
    )
    .map_err(classify_pkcs12_error)?;
    let identities = store
        .entries()
        .filter_map(|(_, entry)| match entry {
            p12_keystore::KeyStoreEntry::PrivateKeyChain(chain) => Some(chain),
            _ => None,
        })
        .collect::<Vec<_>>();
    if identities.len() != 1 {
        return Err(invalid(format!(
            "PKCS12 服务端身份必须且只能包含一个私钥，实际为 {}",
            identities.len()
        )));
    }
    let identity = identities[0];
    normalize_server_identity(
        identity
            .certs()
            .iter()
            .map(|certificate| certificate.as_der().to_vec())
            .collect(),
        Zeroizing::new(identity.key().as_der().to_vec()),
    )
}
