//! 证书生成、解析与校验的纯 Rust 边界。
//!
//! 这里负责 Root/叶子证书、SAN、PKCS#12 和指纹等密码学格式，不负责把私钥写入磁盘。
//! 私钥字节尽量由 `Zeroizing` 持有，解析或密码错误以稳定错误返回；调用方仍必须限制
//! 明文材料的存活时间，并区分“测试代理 CA”与生产支付信任链。

use std::{fmt, io::Cursor, net::IpAddr};

use chrono::{TimeZone, Utc};
use p12_keystore::{KeyStore, KeyStoreEntry, Pkcs12ImportPolicy};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, PKCS_ECDSA_P256_SHA256, SanType,
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
// 终端与桌面时钟可能存在数秒偏差。签发时间向前回退五分钟，避免刚生成的
// 动态叶子证书在终端侧被判定为“尚未生效”。
const CERTIFICATE_CLOCK_SKEW_MINUTES: i64 = 5;
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

pub type ParsedPemClientIdentity = ParsedPemIdentity;
pub use server_identity::ParsedServerIdentity;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCa {
    pub certificate_der: Vec<u8>,
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub metadata: CertificateMetadata,
    canonical_bytes: Vec<u8>,
}

impl TrustedCa {
    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }
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
    /// 解析产品包内固定的测试 Root CA 与签发私钥。
    ///
    /// 固定材料仍经过与运行时生成证书相同的自签名、CA 能力和密钥匹配校验，避免
    /// 打包资源损坏或证书与私钥错配后继续启动监听。
    pub fn load_fixed_root_ca(
        &self,
        certificate_pem: &[u8],
        private_key_pem: &[u8],
    ) -> Result<CertificateBundle, InfrastructureError> {
        let trusted = self.parse_upstream_ca(certificate_pem)?;
        let mut private_key_reader = Cursor::new(private_key_pem);
        let private_key = rustls_pemfile::private_key(&mut private_key_reader)
            .map_err(|error| invalid(format!("固定 Root CA 私钥解析失败：{error}")))?
            .ok_or_else(|| invalid("固定 Root CA 资源不包含私钥"))?;
        let key_pair = KeyPair::try_from(&private_key).map_err(rcgen_error)?;
        let private_key_pkcs8_der = Zeroizing::new(key_pair.serialize_der());
        let metadata = self.validate_root(&trusted.certificate_der, &private_key_pkcs8_der)?;
        Ok(CertificateBundle {
            certificate_der: trusted.certificate_der,
            private_key_pkcs8_der,
            metadata,
        })
    }

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
        params.not_before = ::time::OffsetDateTime::now_utc()
            - ::time::Duration::minutes(CERTIFICATE_CLOCK_SKEW_MINUTES);
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
        params.not_before = ::time::OffsetDateTime::now_utc()
            - ::time::Duration::minutes(CERTIFICATE_CLOCK_SKEW_MINUTES);
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

    /// 解析包含服务端证书链与私钥的 PEM，并把私钥归一化为 PKCS#8。
    pub fn parse_server_identity_pem(
        &self,
        bytes: &[u8],
        password: &str,
    ) -> Result<ParsedServerIdentity, InfrastructureError> {
        server_identity::parse_server_pem(bytes, password)
    }

    pub fn parse_server_identity_pkcs12(
        &self,
        bytes: &[u8],
        password: &str,
    ) -> Result<ParsedServerIdentity, InfrastructureError> {
        server_identity::parse_server_pkcs12(bytes, password)
    }

    /// 解析包含客户端证书链与匹配私钥的组合 PEM。
    pub fn parse_client_identity_pem(
        &self,
        bytes: &[u8],
    ) -> Result<ParsedPemClientIdentity, InfrastructureError> {
        let material = parse_pem_identity(bytes, "PEM 客户端身份")?;
        let certificate_chain_der = material.certificate_chain_der;
        let private_key_pkcs8_der = material.private_key_pkcs8_der;
        let (leaf, chain) = certificate_chain_der
            .split_first()
            .ok_or_else(|| invalid("PEM 客户端身份缺少证书链"))?;
        self.validate_client_identity(leaf, &private_key_pkcs8_der, chain)?;
        Ok(ParsedPemIdentity {
            metadata: metadata(&parse_der(leaf)?)?,
            certificate_chain_der,
            private_key_pkcs8_der,
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
                certificate_chain_der: vec![bytes.to_vec()],
                metadata,
                canonical_bytes: bytes.to_vec(),
            });
        }

        let mut certificate_chain_der = Vec::new();
        let mut metadata_chain = Vec::new();
        for pem in pem_entries {
            if pem.label != "CERTIFICATE" {
                continue;
            }
            metadata_chain.push(validate(&self, &pem.contents)?);
            certificate_chain_der.push(pem.contents);
        }
        let metadata = metadata_chain
            .pop()
            .ok_or_else(|| invalid("证书文件不包含当前有效且受支持的 CA 信任锚"))?;
        let certificate_der = certificate_chain_der
            .last()
            .cloned()
            .ok_or_else(|| invalid("证书文件不包含当前有效且受支持的 CA 信任锚"))?;
        let canonical_bytes = certificate_chain_to_pem(&certificate_chain_der);
        Ok(TrustedCa {
            certificate_der,
            certificate_chain_der,
            metadata,
            canonical_bytes,
        })
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
            return Err(invalid("Proxy 叶子证书不是由固定测试 Root CA 签发"));
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

mod pem_identity;
use pem_identity::{ParsedPemIdentity, parse_pem_identity};

mod server_identity;

mod trust_anchor;
use trust_anchor::certificate_chain_to_pem;

mod validation;
use validation::{
    bundle, certificate_is_ca, classify_pkcs12_error, common_name_dn, extract_sans, invalid,
    is_explicit_legacy_client_trust_anchor, metadata, normalized_san, parse_der, rcgen_error,
    validate_ca, validate_client_chain, validate_digital_signature_usage, validate_key_match,
    validate_server_end_entity, validate_validity, x509_error,
};

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
#[path = "certificates_tests.rs"]
mod certificates_tests;
