//! 证书生成、解析与校验的纯 Rust 边界。
//!
//! 这里负责 Root/叶子证书、SAN、PKCS#12 和指纹等密码学格式，不负责把私钥写入磁盘。
//! 私钥字节尽量由 `Zeroizing` 持有，解析或密码错误以稳定错误返回；调用方仍必须限制
//! 明文材料的存活时间，并区分“测试代理 CA”与生产支付信任链。

use std::{fmt, net::IpAddr};

use chrono::{TimeZone, Utc};
use p12_keystore::{KeyStore, KeyStoreEntry, Pkcs12ImportPolicy};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, PublicKeyData, SanType,
};
use ring::digest::{SHA256, digest};
use x509_parser::{
    certificate::X509Certificate,
    extensions::GeneralName,
    parse_x509_certificate,
    pem::{Pem, parse_x509_pem},
};
use zeroize::{Zeroize, Zeroizing};

use crate::InfrastructureError;

const ROOT_VALIDITY_DAYS: i64 = 3650;
const LEAF_VALIDITY_DAYS: i64 = 825;
pub struct CertificateBundle {
    pub certificate_der: Vec<u8>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub metadata: CertificateMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateMetadata {
    pub subject: String,
    pub issuer: String,
    pub serial_hex: String,
    pub fingerprint_sha256: String,
    pub not_before: String,
    pub not_after: String,
    pub san: Vec<String>,
    pub is_ca: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafCertificateRequest {
    pub common_name: String,
    pub dns_names: Vec<String>,
    pub ip_addresses: Vec<IpAddr>,
}

pub struct ParsedPkcs12 {
    pub certificate_der: Vec<u8>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
    pub chain_der: Vec<Vec<u8>>,
    pub metadata: CertificateMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCa {
    pub certificate_der: Vec<u8>,
    pub metadata: CertificateMetadata,
}

impl fmt::Debug for CertificateBundle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CertificateBundle")
            .field("certificate_der_len", &self.certificate_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl fmt::Debug for ParsedPkcs12 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ParsedPkcs12")
            .field("certificate_der_len", &self.certificate_der.len())
            .field("private_key_pkcs8_der", &"<redacted>")
            .field("chain_certificates", &self.chain_der.len())
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct CertificateService;

impl CertificateService {
    pub fn load_bundled_upstream_ca(
        &self,
        certificates_pem: &[u8],
    ) -> Result<TrustedCa, InfrastructureError> {
        self.parse_upstream_ca(certificates_pem)
    }

    pub fn generate_root_ca(
        &self,
        common_name: &str,
    ) -> Result<CertificateBundle, InfrastructureError> {
        if common_name.trim().is_empty() {
            return Err(invalid("Root CA 名称不能为空"));
        }
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(rcgen_error)?;
        let mut params = CertificateParams::default();
        params.distinguished_name = common_name_dn(common_name);
        params.not_before = ::time::OffsetDateTime::now_utc();
        params.not_after = params.not_before + ::time::Duration::days(ROOT_VALIDITY_DAYS);
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let certificate = params.self_signed(&key).map_err(rcgen_error)?;
        bundle(certificate.der().to_vec(), key.serialize_der())
    }

    pub fn generate_leaf(
        &self,
        root_certificate_der: &[u8],
        root_private_key_pkcs8_der: &[u8],
        request: &LeafCertificateRequest,
    ) -> Result<CertificateBundle, InfrastructureError> {
        if request.common_name.trim().is_empty() {
            return Err(invalid("叶子证书名称不能为空"));
        }
        if request.dns_names.is_empty() && request.ip_addresses.is_empty() {
            return Err(invalid("叶子证书至少需要一个 DNS 或 IP SAN"));
        }
        self.validate_root(root_certificate_der, root_private_key_pkcs8_der)?;
        let root_key = KeyPair::from_pkcs8_der_and_sign_algo(
            &root_private_key_pkcs8_der.into(),
            &PKCS_ECDSA_P256_SHA256,
        )
        .map_err(rcgen_error)?;
        let issuer = Issuer::from_ca_cert_der(&root_certificate_der.into(), root_key)
            .map_err(rcgen_error)?;
        let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(rcgen_error)?;
        let mut params = CertificateParams::default();
        params.distinguished_name = common_name_dn(&request.common_name);
        params.not_before = ::time::OffsetDateTime::now_utc();
        params.not_after = params.not_before + ::time::Duration::days(LEAF_VALIDITY_DAYS);
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.subject_alt_names = request
            .dns_names
            .iter()
            .map(|name| {
                name.as_str()
                    .try_into()
                    .map(SanType::DnsName)
                    .map_err(rcgen_error)
            })
            .chain(
                request
                    .ip_addresses
                    .iter()
                    .copied()
                    .map(|address| Ok(SanType::IpAddress(address))),
            )
            .collect::<Result<Vec<_>, _>>()?;
        let certificate = params.signed_by(&leaf_key, &issuer).map_err(rcgen_error)?;
        bundle(certificate.der().to_vec(), leaf_key.serialize_der())
    }

    pub fn parse_pkcs12(
        &self,
        bytes: &[u8],
        password: &str,
    ) -> Result<ParsedPkcs12, InfrastructureError> {
        let store = KeyStore::from_pkcs12(bytes, password, Pkcs12ImportPolicy::Strict)
            .map_err(classify_pkcs12_error)?;
        let identities = store
            .entries()
            .filter_map(|(_, entry)| match entry {
                KeyStoreEntry::PrivateKeyChain(chain) => Some(chain),
                _ => None,
            })
            .collect::<Vec<_>>();
        if identities.len() != 1 {
            return Err(invalid(format!(
                "PKCS12 必须且只能包含一个私钥身份，实际为 {}",
                identities.len()
            )));
        }
        let identity = identities[0];
        let (certificate, chain) = identity
            .certs()
            .split_first()
            .ok_or_else(|| invalid("PKCS12 私钥身份缺少证书链"))?;
        validate_key_match(certificate.as_der(), identity.key().as_der())?;
        validate_validity(&parse_der(certificate.as_der())?)?;
        validate_client_chain(certificate.as_der(), chain)?;
        Ok(ParsedPkcs12 {
            certificate_der: certificate.as_der().to_vec(),
            private_key_pkcs8_der: Zeroizing::new(identity.key().as_der().to_vec()),
            chain_der: chain.iter().map(|cert| cert.as_der().to_vec()).collect(),
            metadata: metadata(&parse_der(certificate.as_der())?)?,
        })
    }

    pub fn parse_ca(&self, bytes: &[u8]) -> Result<CertificateMetadata, InfrastructureError> {
        let der = match parse_x509_pem(bytes) {
            Ok((_, pem)) => pem.contents,
            Err(_) => bytes.to_vec(),
        };
        let certificate = parse_der(&der)?;
        validate_ca(&certificate)?;
        metadata(&certificate)
    }

    /// 读取 DER 或 PEM 中第一张证书的公开元数据。
    ///
    /// 该方法只用于详情展示，不替代具体用途的 CA、客户端身份或服务端身份校验。
    /// 调用方仍必须在真正建立 TLS 前走对应的严格校验路径。
    pub fn inspect_certificate(
        &self,
        bytes: &[u8],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        let der = match Pem::iter_from_buffer(bytes)
            .filter_map(Result::ok)
            .find(|pem| pem.label == "CERTIFICATE")
        {
            Some(pem) => pem.contents,
            None => bytes.to_vec(),
        };
        metadata(&parse_der(&der)?)
    }

    pub fn parse_upstream_ca(&self, bytes: &[u8]) -> Result<TrustedCa, InfrastructureError> {
        self.parse_trust_anchor(bytes, Self::validate_ca_der)
    }

    /// 解析用于验证下游客户端证书的信任锚。
    ///
    /// 部分既有终端证书链的自签名根 CA 只有 `BasicConstraints CA:TRUE`，没有
    /// `KeyUsage` 扩展。它们可以作为显式配置的信任锚，但不能按现代上游服务器
    /// CA 的严格策略校验，因此必须走专用兼容入口。
    pub fn parse_client_trust_anchor(
        &self,
        bytes: &[u8],
    ) -> Result<TrustedCa, InfrastructureError> {
        self.parse_trust_anchor(bytes, Self::validate_client_trust_anchor_der)
    }

    fn parse_trust_anchor(
        self,
        bytes: &[u8],
        validate: fn(&Self, &[u8]) -> Result<CertificateMetadata, InfrastructureError>,
    ) -> Result<TrustedCa, InfrastructureError> {
        let pem_entries = Pem::iter_from_buffer(bytes)
            .collect::<Result<Vec<_>, _>>()
            .map_err(x509_error)?;
        if pem_entries.is_empty() {
            let metadata = validate(&self, bytes)?;
            return Ok(TrustedCa {
                certificate_der: bytes.to_vec(),
                metadata,
            });
        }

        for pem in pem_entries.into_iter().rev() {
            if pem.label != "CERTIFICATE" {
                continue;
            }
            if let Ok(metadata) = validate(&self, &pem.contents) {
                return Ok(TrustedCa {
                    certificate_der: pem.contents,
                    metadata,
                });
            }
        }
        Err(invalid("证书文件不包含当前有效且受支持的 CA 信任锚"))
    }

    pub fn validate_root(
        &self,
        certificate_der: &[u8],
        private_key_der: &[u8],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        let certificate = parse_der(certificate_der)?;
        validate_key_match(certificate_der, private_key_der)?;
        validate_ca(&certificate)?;
        if certificate.subject() != certificate.issuer()
            || certificate.verify_signature(None).is_err()
        {
            return Err(invalid("Root CA 必须是有效的自签名证书"));
        }
        metadata(&certificate)
    }

    pub fn validate_leaf(
        &self,
        root_certificate_der: &[u8],
        certificate_der: &[u8],
        private_key_der: &[u8],
        required_sans: &[String],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        let root = parse_der(root_certificate_der)?;
        validate_ca(&root)?;
        let certificate = parse_der(certificate_der)?;
        validate_key_match(certificate_der, private_key_der)?;
        validate_validity(&certificate)?;
        if certificate_is_ca(&certificate)? {
            return Err(invalid("Proxy 叶子证书不得声明 CA 能力"));
        }
        validate_digital_signature_usage(&certificate, "Proxy 叶子证书")?;
        if certificate.issuer() != root.subject()
            || certificate
                .verify_signature(Some(root.public_key()))
                .is_err()
        {
            return Err(invalid("Proxy 叶子证书不是由当前安装实例的 Root CA 签发"));
        }
        let eku = certificate
            .extended_key_usage()
            .map_err(x509_error)?
            .ok_or_else(|| invalid("Proxy 叶子证书缺少 serverAuth EKU"))?;
        if !eku.value.server_auth {
            return Err(invalid("Proxy 叶子证书缺少 serverAuth EKU"));
        }
        let actual_sans = extract_sans(&certificate)?;
        for required in required_sans {
            let required = normalized_san(required);
            if !actual_sans
                .iter()
                .any(|actual| normalized_san(actual) == required)
            {
                return Err(invalid(format!("Proxy 叶子证书缺少 SAN：{required}")));
            }
        }
        metadata(&certificate)
    }

    pub fn validate_ca_der(
        &self,
        certificate_der: &[u8],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        let certificate = parse_der(certificate_der)?;
        validate_ca(&certificate)?;
        metadata(&certificate)
    }

    pub fn validate_client_trust_anchor_der(
        &self,
        certificate_der: &[u8],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        let certificate = parse_der(certificate_der)?;
        if validate_ca(&certificate).is_err()
            && !is_explicit_legacy_client_trust_anchor(&certificate)?
        {
            return Err(invalid("客户端信任锚必须是有效 CA 或受支持的旧式自签名 CA"));
        }
        metadata(&certificate)
    }

    pub fn validate_client_identity(
        &self,
        certificate_der: &[u8],
        private_key_der: &[u8],
        chain_der: &[Vec<u8>],
    ) -> Result<CertificateMetadata, InfrastructureError> {
        validate_key_match(certificate_der, private_key_der)?;
        validate_validity(&parse_der(certificate_der)?)?;
        let chain = chain_der
            .iter()
            .map(|der| p12_keystore::Certificate::from_der(der).map_err(classify_pkcs12_error))
            .collect::<Result<Vec<_>, _>>()?;
        validate_client_chain(certificate_der, &chain)?;
        metadata(&parse_der(certificate_der)?)
    }
}

fn common_name_dn(common_name: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    name
}

fn bundle(
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
) -> Result<CertificateBundle, InfrastructureError> {
    Ok(CertificateBundle {
        metadata: metadata(&parse_der(&certificate_der)?)?,
        certificate_der,
        private_key_pkcs8_der: Zeroizing::new(private_key_der),
    })
}

fn parse_der(bytes: &[u8]) -> Result<X509Certificate<'_>, InfrastructureError> {
    let (remaining, certificate) = parse_x509_certificate(bytes).map_err(x509_error)?;
    if !remaining.is_empty() {
        return Err(invalid("X.509 DER 包含尾随数据"));
    }
    Ok(certificate)
}

fn metadata(certificate: &X509Certificate<'_>) -> Result<CertificateMetadata, InfrastructureError> {
    Ok(CertificateMetadata {
        subject: certificate.subject().to_string(),
        issuer: certificate.issuer().to_string(),
        serial_hex: certificate.raw_serial_as_string(),
        fingerprint_sha256: fingerprint(certificate.as_raw()),
        not_before: validity_text(certificate.validity().not_before.timestamp())?,
        not_after: validity_text(certificate.validity().not_after.timestamp())?,
        san: extract_sans(certificate)?,
        is_ca: certificate_is_ca(certificate)?,
    })
}

fn validate_key_match(
    certificate_der: &[u8],
    private_key_der: &[u8],
) -> Result<(), InfrastructureError> {
    let certificate = parse_der(certificate_der)?;
    let key = KeyPair::try_from(private_key_der).map_err(rcgen_error)?;
    if certificate.public_key().raw == key.subject_public_key_info() {
        Ok(())
    } else {
        Err(invalid("证书与私钥不匹配"))
    }
}

fn validate_validity(certificate: &X509Certificate<'_>) -> Result<(), InfrastructureError> {
    if certificate.validity().is_valid() {
        Ok(())
    } else {
        Err(invalid("证书不在有效期内"))
    }
}

fn validate_ca(certificate: &X509Certificate<'_>) -> Result<(), InfrastructureError> {
    validate_validity(certificate)?;
    let is_ca = certificate_is_ca(certificate)?;
    let key_usage = certificate.key_usage().map_err(x509_error)?;
    if is_ca && key_usage.is_some_and(|usage| usage.value.key_cert_sign()) {
        Ok(())
    } else {
        Err(invalid("证书缺少 CA Basic Constraints 或证书签发用途"))
    }
}

fn certificate_is_ca(certificate: &X509Certificate<'_>) -> Result<bool, InfrastructureError> {
    Ok(certificate
        .basic_constraints()
        .map_err(x509_error)?
        .is_some_and(|constraints| constraints.value.ca))
}

fn extract_sans(certificate: &X509Certificate<'_>) -> Result<Vec<String>, InfrastructureError> {
    Ok(certificate
        .subject_alternative_name()
        .map_err(x509_error)?
        .map_or_else(Vec::new, |extension| {
            extension
                .value
                .general_names
                .iter()
                .filter_map(|name| match name {
                    GeneralName::DNSName(value) => Some(format!("DNS:{value}")),
                    GeneralName::IPAddress(bytes) => match *bytes {
                        [a, b, c, d] => Some(format!("IP:{a}.{b}.{c}.{d}")),
                        octets if octets.len() == 16 => {
                            let octets: [u8; 16] = octets.try_into().ok()?;
                            Some(format!("IP:{}", std::net::Ipv6Addr::from(octets)))
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect()
        }))
}

fn validate_client_chain(
    certificate_der: &[u8],
    chain: &[p12_keystore::Certificate],
) -> Result<(), InfrastructureError> {
    let certificate = parse_der(certificate_der)?;
    validate_client_end_entity(&certificate)?;
    if chain.is_empty() {
        return Err(invalid("PKCS12 缺少可验证的 CA 证书链"));
    }
    let mut current_der = certificate_der;
    for (index, issuer) in chain.iter().enumerate() {
        let current = parse_der(current_der)?;
        let issuer_x509 = parse_der(issuer.as_der())?;
        if current.issuer() != issuer_x509.subject()
            || current
                .verify_signature(Some(issuer_x509.public_key()))
                .is_err()
        {
            return Err(invalid("PKCS12 证书链签名无效"));
        }
        if validate_ca(&issuer_x509).is_err()
            && !(index + 1 == chain.len() && is_legacy_self_signed_trust_anchor(&issuer_x509)?)
        {
            return Err(invalid("证书缺少 CA Basic Constraints 或证书签发用途"));
        }
        current_der = issuer.as_der();
    }
    Ok(())
}

fn is_legacy_self_signed_trust_anchor(
    certificate: &X509Certificate<'_>,
) -> Result<bool, InfrastructureError> {
    validate_validity(certificate)?;
    let basic_constraints = certificate.basic_constraints().map_err(x509_error)?;
    if basic_constraints.is_some_and(|constraints| !constraints.value.ca)
        || certificate.key_usage().map_err(x509_error)?.is_some()
        || certificate.subject() != certificate.issuer()
    {
        return Ok(false);
    }
    Ok(certificate
        .verify_signature(Some(certificate.public_key()))
        .is_ok())
}

/// 判断用户显式选择的旧式客户端信任锚。
///
/// 信任锚的自签名只用于封装公钥，并不参与到终端证书的信任计算；因此这里不要求
/// 当前密码学提供方能够验证历史 SHA-1 自签名。客户端终端证书到该根的签名仍由
/// rustls/webpki 在实际握手时校验。
fn is_explicit_legacy_client_trust_anchor(
    certificate: &X509Certificate<'_>,
) -> Result<bool, InfrastructureError> {
    validate_validity(certificate)?;
    let basic_constraints = certificate.basic_constraints().map_err(x509_error)?;
    Ok(
        basic_constraints.is_some_and(|constraints| constraints.value.ca)
            && certificate.key_usage().map_err(x509_error)?.is_none()
            && certificate.subject() == certificate.issuer(),
    )
}

fn validate_client_end_entity(
    certificate: &X509Certificate<'_>,
) -> Result<(), InfrastructureError> {
    validate_validity(certificate)?;
    if certificate_is_ca(certificate)? {
        return Err(invalid("PKCS12 客户端身份必须是非 CA 终端证书"));
    }
    validate_digital_signature_usage(certificate, "PKCS12 客户端证书")?;
    let eku = certificate
        .extended_key_usage()
        .map_err(x509_error)?
        .ok_or_else(|| invalid("PKCS12 客户端证书缺少 clientAuth EKU"))?;
    if !eku.value.client_auth {
        return Err(invalid("PKCS12 客户端证书缺少 clientAuth EKU"));
    }
    Ok(())
}

fn validate_digital_signature_usage(
    certificate: &X509Certificate<'_>,
    label: &str,
) -> Result<(), InfrastructureError> {
    let key_usage = certificate
        .key_usage()
        .map_err(x509_error)?
        .ok_or_else(|| invalid(format!("{label}缺少 DigitalSignature KeyUsage")))?;
    if !key_usage.value.digital_signature() {
        return Err(invalid(format!("{label}缺少 DigitalSignature KeyUsage")));
    }
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn classify_pkcs12_error(error: p12_keystore::error::Error) -> InfrastructureError {
    use p12_keystore::error::Error;
    match error {
        Error::MacError(_) | Error::UnpadError => InfrastructureError::Pkcs12PasswordInvalid,
        Error::UnsupportedContentType
        | Error::UnsupportedCertificateType
        | Error::UnsupportedEncryptionScheme
        | Error::UnsupportedMacAlgorithm => invalid(format!("不支持的 PKCS12 格式：{error}")),
        _ => invalid(format!("PKCS12 内容无效：{error}")),
    }
}

fn normalized_san(value: &str) -> &str {
    value.trim_start_matches("DNS:").trim_start_matches("IP:")
}

fn fingerprint(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn validity_text(timestamp: i64) -> Result<String, InfrastructureError> {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%b %e %H:%M:%S %Y GMT").to_string())
        .ok_or_else(|| invalid("证书有效期超出支持范围"))
}

fn rcgen_error(error: impl fmt::Display) -> InfrastructureError {
    invalid(format!("证书生成或密钥校验失败：{error}"))
}

fn x509_error(error: impl fmt::Debug) -> InfrastructureError {
    invalid(format!("X.509 校验失败：{error:?}"))
}

fn invalid(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::CertificateInvalid {
        message: message.into(),
    }
}

impl Drop for CertificateBundle {
    fn drop(&mut self) {
        self.certificate_der.zeroize();
    }
}

impl Drop for ParsedPkcs12 {
    fn drop(&mut self) {
        self.certificate_der.zeroize();
        for certificate in &mut self.chain_der {
            certificate.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p12_keystore::{Certificate, KeyStoreEntry, PrivateKey, PrivateKeyChain};

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
}
