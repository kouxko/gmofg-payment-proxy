use super::{
    Arc, CertificateDer, ClientConfig, ClientTlsAdapter, DigitallySignedStruct,
    DynamicServerIdentityResolver, ErrorCode, GeneralName, HandshakeSignatureValid, PrivateKeyDer,
    PrivatePkcs8KeyDer, ProxyError, Result, ReverseDownstreamTls, ReverseUpstreamTls,
    RootCertStore, ServerCertVerified, ServerCertVerifier, ServerConfig, ServerName,
    SignatureScheme, TLS12, TlsAcceptor, UnixTime, WebPkiClientVerifier, WebPkiServerVerifier,
    certified_key, parse_x509_certificate,
};

pub(super) fn build_server_acceptor(settings: &ReverseDownstreamTls) -> Result<TlsAcceptor> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&TLS12])
        .map_err(config_error)?;
    let config = if settings.client_trust_der.is_empty() {
        if settings.client_authentication_required {
            return Err(ProxyError::new(
                ErrorCode::ConfigInvalid,
                "required downstream client authentication has no trust anchor",
            ));
        }
        builder.with_no_client_auth()
    } else {
        let roots = root_store(&settings.client_trust_der)?;
        let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider);
        let verifier = if settings.client_authentication_required {
            verifier
        } else {
            verifier.allow_unauthenticated()
        }
        .build()
        .map_err(config_error)?;
        builder.with_client_cert_verifier(verifier)
    };
    let fallback = certified_key(&settings.server_identity)?;
    let config = match &settings.dynamic_server_identity {
        Some(authority) => config.with_cert_resolver(Arc::new(DynamicServerIdentityResolver::new(
            Arc::clone(authority),
            settings.dynamic_server_name_allowlist.clone(),
            fallback,
        ))),
        None => config.with_cert_resolver(Arc::new(rustls::sign::SingleCertAndKey::from(fallback))),
    };
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub(crate) fn build_client_connector(settings: &ReverseUpstreamTls) -> Result<ClientTlsAdapter> {
    let mut roots = root_store(&settings.server_trust_der)?;
    if settings.server_trust_der.is_empty() {
        let native = rustls_native_certs::load_native_certs();
        for certificate in native.certs {
            roots.add(certificate).map_err(config_error)?;
        }
        if roots.is_empty() {
            return Err(ProxyError::new(
                ErrorCode::CertificateNotReady,
                format!(
                    "system trust store contains no usable roots: {:?}",
                    native.errors
                ),
            ));
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let chain_verifier = if settings.verify_hostname {
        None
    } else {
        Some(
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots.clone()), provider.clone())
                .build()
                .map_err(config_error)?,
        )
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS12])
        .map_err(config_error)?
        .with_root_certificates(roots);
    let mut config = if let Some(identity) = &settings.client_identity {
        builder
            .with_client_auth_cert(
                certificate_chain(&identity.certificate_chain_der),
                private_key(&identity.private_key_pkcs8_der),
            )
            .map_err(config_error)?
    } else {
        builder.with_no_client_auth()
    };
    if let Some(inner) = chain_verifier {
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(ChainOnlyServerVerifier { inner }));
    }
    Ok(ClientTlsAdapter::from_config(
        config,
        settings.verify_hostname,
        settings.client_identity.is_some(),
    ))
}

/// 保留 `WebPKI` 的证书链、有效期、用途与签名验证，只把目标主机名替换为证书自身首个
/// DNS/IP SAN。它不会退化为接受任意证书。
#[derive(Debug)]
struct ChainOnlyServerVerifier {
    inner: Arc<dyn ServerCertVerifier>,
}

impl ServerCertVerifier for ChainOnlyServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let certificate_name = certificate_san_name(end_entity.as_ref()).map_err(|_| {
            rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            )
        })?;
        self.inner.verify_server_cert(
            end_entity,
            intermediates,
            &certificate_name,
            ocsp_response,
            now,
        )
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls12_signature(message, certificate, signature)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        self.inner
            .verify_tls13_signature(message, certificate, signature)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn certificate_san_name(certificate_der: &[u8]) -> Result<ServerName<'static>> {
    let (_, certificate) = parse_x509_certificate(certificate_der).map_err(|error| {
        ProxyError::new(
            ErrorCode::CertificateInvalid,
            format!("upstream certificate is invalid: {error}"),
        )
    })?;
    let names = certificate
        .subject_alternative_name()
        .map_err(|error| {
            ProxyError::new(
                ErrorCode::CertificateInvalid,
                format!("upstream certificate SAN is invalid: {error}"),
            )
        })?
        .ok_or_else(|| {
            ProxyError::new(
                ErrorCode::CertificateInvalid,
                "hostname verification can only be disabled for a certificate containing SAN",
            )
        })?;
    for name in &names.value.general_names {
        match name {
            GeneralName::DNSName(name) => {
                return ServerName::try_from((*name).to_owned()).map_err(config_error);
            }
            GeneralName::IPAddress(bytes) if bytes.len() == 4 => {
                return Ok(ServerName::IpAddress(
                    std::net::Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]).into(),
                ));
            }
            GeneralName::IPAddress(bytes) if bytes.len() == 16 => {
                let octets: [u8; 16] = (*bytes).try_into().map_err(|_| {
                    ProxyError::new(ErrorCode::CertificateInvalid, "invalid IPv6 SAN length")
                })?;
                return Ok(ServerName::IpAddress(
                    std::net::Ipv6Addr::from(octets).into(),
                ));
            }
            _ => {}
        }
    }
    Err(ProxyError::new(
        ErrorCode::CertificateInvalid,
        "upstream certificate SAN contains no DNS or IP identity",
    ))
}

fn root_store(certificates: &[Vec<u8>]) -> Result<RootCertStore> {
    let mut roots = RootCertStore::empty();
    for certificate in certificates {
        roots
            .add(CertificateDer::from(certificate.clone()))
            .map_err(config_error)?;
    }
    Ok(roots)
}

fn certificate_chain(certificates: &[Vec<u8>]) -> Vec<CertificateDer<'static>> {
    certificates
        .iter()
        .cloned()
        .map(CertificateDer::from)
        .collect()
}

fn private_key(bytes: &[u8]) -> PrivateKeyDer<'static> {
    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(bytes.to_vec()))
}

pub(super) fn config_error(error: impl std::fmt::Display) -> ProxyError {
    ProxyError::new(ErrorCode::CertificateInvalid, error.to_string())
}
