use super::{
    CertificateBundle, CertificateMetadata, DistinguishedName, DnType, GeneralName,
    InfrastructureError, KeyPair, PublicKeyData, SHA256, TimeZone, Utc, X509Certificate, Zeroizing,
    digest, fmt, parse_x509_certificate,
};

pub(super) fn common_name_dn(common_name: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common_name);
    name
}

pub(super) fn bundle(
    certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
) -> Result<CertificateBundle, InfrastructureError> {
    Ok(CertificateBundle {
        metadata: metadata(&parse_der(&certificate_der)?)?,
        certificate_der,
        private_key_pkcs8_der: Zeroizing::new(private_key_der),
    })
}

pub(super) fn parse_der(bytes: &[u8]) -> Result<X509Certificate<'_>, InfrastructureError> {
    let (remaining, certificate) = parse_x509_certificate(bytes).map_err(x509_error)?;
    if !remaining.is_empty() {
        return Err(invalid("X.509 DER 包含尾随数据"));
    }
    Ok(certificate)
}

pub(super) fn metadata(
    certificate: &X509Certificate<'_>,
) -> Result<CertificateMetadata, InfrastructureError> {
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

pub(super) fn validate_key_match(
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

pub(super) fn validate_validity(
    certificate: &X509Certificate<'_>,
) -> Result<(), InfrastructureError> {
    if certificate.validity().is_valid() {
        Ok(())
    } else {
        Err(invalid("证书不在有效期内"))
    }
}

pub(super) fn validate_ca(certificate: &X509Certificate<'_>) -> Result<(), InfrastructureError> {
    validate_validity(certificate)?;
    let is_ca = certificate_is_ca(certificate)?;
    let key_usage = certificate.key_usage().map_err(x509_error)?;
    if is_ca && key_usage.is_some_and(|usage| usage.value.key_cert_sign()) {
        Ok(())
    } else {
        Err(invalid("证书缺少 CA Basic Constraints 或证书签发用途"))
    }
}

pub(super) fn certificate_is_ca(
    certificate: &X509Certificate<'_>,
) -> Result<bool, InfrastructureError> {
    Ok(certificate
        .basic_constraints()
        .map_err(x509_error)?
        .is_some_and(|constraints| constraints.value.ca))
}

pub(super) fn extract_sans(
    certificate: &X509Certificate<'_>,
) -> Result<Vec<String>, InfrastructureError> {
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

pub(super) fn validate_client_chain(
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

pub(super) fn is_legacy_self_signed_trust_anchor(
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
pub(super) fn is_explicit_legacy_client_trust_anchor(
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

pub(super) fn validate_client_end_entity(
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

pub(super) fn validate_server_end_entity(
    certificate_der: &[u8],
    private_key_der: &[u8],
) -> Result<(), InfrastructureError> {
    let certificate = parse_der(certificate_der)?;
    validate_key_match(certificate_der, private_key_der)?;
    validate_validity(&certificate)?;
    if certificate_is_ca(&certificate)? {
        return Err(invalid("PEM 服务端身份必须是非 CA 终端证书"));
    }
    validate_digital_signature_usage(&certificate, "PEM 服务端证书")?;
    let eku = certificate
        .extended_key_usage()
        .map_err(x509_error)?
        .ok_or_else(|| invalid("PEM 服务端证书缺少 serverAuth EKU"))?;
    if !eku.value.server_auth {
        return Err(invalid("PEM 服务端证书缺少 serverAuth EKU"));
    }
    Ok(())
}

pub(super) fn validate_server_chain(
    certificate_chain_der: &[Vec<u8>],
    private_key_der: &[u8],
) -> Result<(), InfrastructureError> {
    let leaf = certificate_chain_der
        .first()
        .ok_or_else(|| invalid("PEM 服务端身份缺少证书链"))?;
    validate_server_end_entity(leaf, private_key_der)?;

    for pair in certificate_chain_der.windows(2) {
        let certificate = parse_der(&pair[0])?;
        let issuer = parse_der(&pair[1])?;
        validate_ca(&issuer)?;
        if certificate.issuer() != issuer.subject()
            || certificate
                .verify_signature(Some(issuer.public_key()))
                .is_err()
        {
            return Err(invalid("PEM 服务端证书链签发关系或顺序无效"));
        }
    }

    if let Some(last) = certificate_chain_der.last() {
        let trust_anchor = parse_der(last)?;
        validate_validity(&trust_anchor)?;
        if trust_anchor.subject() == trust_anchor.issuer()
            && trust_anchor.verify_signature(None).is_err()
        {
            return Err(invalid("PEM 服务端证书链包含无效的自签名根证书"));
        }
    }
    Ok(())
}

pub(super) fn validate_digital_signature_usage(
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
pub(super) fn classify_pkcs12_error(error: p12_keystore::error::Error) -> InfrastructureError {
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

pub(super) fn normalized_san(value: &str) -> &str {
    value.trim_start_matches("DNS:").trim_start_matches("IP:")
}

pub(super) fn fingerprint(bytes: &[u8]) -> String {
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

pub(super) fn validity_text(timestamp: i64) -> Result<String, InfrastructureError> {
    Utc.timestamp_opt(timestamp, 0)
        .single()
        .map(|time| time.format("%b %e %H:%M:%S %Y GMT").to_string())
        .ok_or_else(|| invalid("证书有效期超出支持范围"))
}

pub(super) fn rcgen_error(error: impl fmt::Display) -> InfrastructureError {
    invalid(format!("证书生成或密钥校验失败：{error}"))
}

pub(super) fn x509_error(error: impl fmt::Debug) -> InfrastructureError {
    invalid(format!("X.509 校验失败：{error:?}"))
}

pub(super) fn invalid(message: impl Into<String>) -> InfrastructureError {
    InfrastructureError::CertificateInvalid {
        message: message.into(),
    }
}
