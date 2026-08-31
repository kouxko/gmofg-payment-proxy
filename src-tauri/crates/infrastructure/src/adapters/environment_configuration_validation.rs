//! Concrete, side-effect-free environment candidate validation probes.

use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use intercept_proxy_application::{
    AppError, AppResult, EnvironmentMaterialProbe, EnvironmentMaterialProbeKind,
    EnvironmentValidationLayer, EnvironmentValidationLayerPort, EnvironmentValidationLayerRequest,
    EnvironmentValidationStatus, ExternalPackageApplicationPort, ProtocolPackageSourceViewModel,
    ProtocolPackageValidationViewModel,
};
use tokio::net::{TcpStream, lookup_host};
use zeroize::Zeroizing;

use crate::CertificateService;
use intercept_proxy_runtime::{
    SocketEndpoint, SocketRelayConfig, SocketRelaySecurity, SocketRelayService, SocketTlsIdentity,
    SocketUpstreamTlsConfig,
};

use super::listener_runtime::ListenerMitmAuthorityProvider;

mod probes;
use probes::run_bounded_probes;

const INSTALLATION_ROOT_SELECTOR: &str = "installation:root-ca";
const MAX_CONCURRENT_NETWORK_PROBES: usize = 4;
const MAX_UPSTREAM_TARGETS: usize = 16;

pub(crate) struct EnvironmentConfigurationValidationAdapter {
    external_packages: Arc<dyn ExternalPackageApplicationPort>,
    installation_tls: Arc<dyn ListenerMitmAuthorityProvider>,
    certificates: CertificateService,
}

impl fmt::Debug for EnvironmentConfigurationValidationAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EnvironmentConfigurationValidationAdapter")
            .field("material_validation", &"ephemeral")
            .field("package_projection", &"get-only")
            .field("network_probe_limit", &MAX_CONCURRENT_NETWORK_PROBES)
            .finish_non_exhaustive()
    }
}

impl EnvironmentConfigurationValidationAdapter {
    pub(crate) fn new(
        external_packages: Arc<dyn ExternalPackageApplicationPort>,
        installation_tls: Arc<dyn ListenerMitmAuthorityProvider>,
    ) -> Self {
        Self {
            external_packages,
            installation_tls,
            certificates: CertificateService,
        }
    }

    fn validate_materials(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        let mut result = Ok(EnvironmentValidationStatus::Passed);
        request.visit_materials(|material| {
            if result.is_ok() {
                result = self.validate_material(material);
            }
        });
        result
    }

    fn validate_material(
        &self,
        material: EnvironmentMaterialProbe<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        match material.kind() {
            EnvironmentMaterialProbeKind::Certificate => {
                self.validate_certificate_material(material)?;
            }
            EnvironmentMaterialProbeKind::Secret => {
                if material.role() != "proxy_basic_auth"
                    || material.username().is_none_or(str::is_empty)
                    || material.content().is_empty()
                {
                    return Err(stable_error("SECRET_VALUE_INVALID"));
                }
            }
        }
        Ok(EnvironmentValidationStatus::Passed)
    }

    fn validate_certificate_material(
        &self,
        material: EnvironmentMaterialProbe<'_>,
    ) -> AppResult<()> {
        self.validate_certificate_input(
            material.role(),
            material.encoding(),
            material.content(),
            material.password(),
        )
    }

    fn validate_certificate_input(
        &self,
        role: &str,
        encoding: Option<&str>,
        content: &str,
        password: Option<&str>,
    ) -> AppResult<()> {
        let encoding = encoding.ok_or_else(|| stable_error("CERTIFICATE_PARSE_FAILED"))?;
        let decoded = decode_certificate_material(encoding, content)?;
        let password = password.unwrap_or_default();
        let parsed = match (role, encoding) {
            ("downstream_server_identity", "pem") => self
                .certificates
                .parse_server_identity_pem(&decoded, password)
                .map(|_| ()),
            ("downstream_server_identity", "pkcs12_base64") => self
                .certificates
                .parse_server_identity_pkcs12(&decoded, password)
                .map(|_| ()),
            ("downstream_client_trust", "pem" | "base64_der") => self
                .certificates
                .parse_client_trust_anchor(&decoded)
                .map(|_| ()),
            ("upstream_client_identity", "pem") => self
                .certificates
                .parse_client_identity_pem(&decoded)
                .map(|_| ()),
            ("upstream_client_identity", "pkcs12_base64") => self
                .certificates
                .parse_pkcs12(&decoded, password)
                .map(|_| ()),
            ("upstream_server_trust", "pem" | "base64_der") => {
                self.certificates.parse_upstream_ca(&decoded).map(|_| ())
            }
            _ => return Err(stable_error("CERTIFICATE_ROLE_MISMATCH")),
        };
        parsed.map_err(|_| stable_error("CERTIFICATE_PARSE_FAILED"))
    }

    async fn validate_packages(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        self.validate_package_refs(request.exact_package_refs())
            .await
    }

    async fn validate_package_refs(
        &self,
        packages: &[intercept_proxy_application::ProtocolPackageRef],
    ) -> AppResult<EnvironmentValidationStatus> {
        for package in packages {
            let projection = self
                .external_packages
                .get(package)
                .await?
                .ok_or_else(|| stable_error("PROTOCOL_PACKAGE_NOT_INSTALLED"))?;
            if !projection.enabled {
                return Err(stable_error("PROTOCOL_PACKAGE_DISABLED"));
            }
            if matches!(
                projection.validation,
                ProtocolPackageValidationViewModel::Invalid { .. }
            ) {
                return Err(stable_error("PROTOCOL_PACKAGE_INCOMPATIBLE"));
            }
            if matches!(
                projection.source,
                ProtocolPackageSourceViewModel::External { online: false }
            ) {
                return Err(stable_error("EXTERNAL_PACKAGE_OFFLINE"));
            }
        }
        Ok(EnvironmentValidationStatus::Passed)
    }

    async fn validate_dns_tcp(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        if request.dns_tcp_targets().len() > MAX_UPSTREAM_TARGETS {
            return Err(stable_error("DTO_LIMIT_EXCEEDED"));
        }
        let targets = request
            .dns_tcp_targets()
            .iter()
            .map(|target| (target.host().to_owned(), target.port()))
            .collect::<Vec<_>>();
        self.probe_dns_tcp(targets).await
    }

    async fn probe_dns_tcp(
        &self,
        mut targets: Vec<(String, u16)>,
    ) -> AppResult<EnvironmentValidationStatus> {
        if targets.len() > MAX_UPSTREAM_TARGETS {
            return Err(stable_error("DTO_LIMIT_EXCEEDED"));
        }
        targets.sort();
        run_bounded_probes(
            targets,
            MAX_CONCURRENT_NETWORK_PROBES,
            |target| async move {
                let resolved = lookup_host((target.0.as_str(), target.1))
                    .await
                    .map_err(|_| stable_error("VALIDATION_LAYER_FAILED"))?
                    .collect::<Vec<_>>();
                if resolved.is_empty() {
                    return Err(stable_error("VALIDATION_LAYER_FAILED"));
                }
                for address in resolved {
                    if TcpStream::connect(address).await.is_ok() {
                        return Ok(());
                    }
                }
                Err(stable_error("VALIDATION_LAYER_FAILED"))
            },
        )
        .await?;
        Ok(EnvironmentValidationStatus::Passed)
    }

    async fn validate_tls_mtls(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        let root_status = self.validate_installation_roots(request).await?;
        let mut targets = request.tls_mtls_targets().iter().collect::<Vec<_>>();
        targets.sort_by_key(|target| {
            (
                target.host(),
                target.port(),
                target.server_name(),
                target.upstream_ca_alias(),
                target.client_identity_alias(),
            )
        });
        if targets.is_empty() {
            return Ok(root_status);
        }
        let mut probes = Vec::with_capacity(targets.len());
        for target in targets {
            let server_trust_der = self.server_trust(request, target.upstream_ca_alias())?;
            let client_identity = self.client_identity(request, target.client_identity_alias())?;
            let service = build_tls_probe(TlsProbeInput {
                host: target.host().to_owned(),
                port: target.port(),
                server_name: target.server_name().map(str::to_owned),
                server_trust_der,
                client_identity,
                verify_hostname: target.verify_hostname(),
            })?;
            probes.push((service, target.verify_hostname()));
        }
        run_bounded_probes(
            probes,
            MAX_CONCURRENT_NETWORK_PROBES,
            |(service, verify_hostname)| async move {
                let result = service
                    .test_upstream_connection()
                    .await
                    .map_err(|_| stable_error("VALIDATION_LAYER_FAILED"))?;
                let evidence = result
                    .tls
                    .ok_or_else(|| stable_error("VALIDATION_LAYER_FAILED"))?;
                if verify_hostname && !evidence.hostname_verification_enabled {
                    return Err(stable_error("VALIDATION_LAYER_FAILED"));
                }
                Ok(())
            },
        )
        .await?;
        Ok(EnvironmentValidationStatus::Passed)
    }

    fn server_trust(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
        alias: Option<&str>,
    ) -> AppResult<Vec<Vec<u8>>> {
        let Some(alias) = alias else {
            return Ok(Vec::new());
        };
        let mut trust = None;
        request.visit_materials(|material| {
            if trust.is_none() && material.alias() == alias {
                trust = Some(
                    if material.kind() == EnvironmentMaterialProbeKind::Certificate
                        && material.role() == "upstream_server_trust"
                    {
                        self.parse_server_trust(material)
                    } else {
                        Err(stable_error("MATERIAL_ALIAS_TYPE_MISMATCH"))
                    },
                );
            }
        });
        trust.ok_or_else(|| stable_error("MATERIAL_ALIAS_MISSING"))?
    }

    fn parse_server_trust(
        &self,
        material: EnvironmentMaterialProbe<'_>,
    ) -> AppResult<Vec<Vec<u8>>> {
        let encoding = material
            .encoding()
            .ok_or_else(|| stable_error("CERTIFICATE_PARSE_FAILED"))?;
        if !matches!(encoding, "pem" | "base64_der") {
            return Err(stable_error("CERTIFICATE_ROLE_MISMATCH"));
        }
        let decoded = decode_certificate_material(encoding, material.content())?;
        self.certificates
            .parse_upstream_ca(&decoded)
            .map(|trusted| trusted.certificate_chain_der)
            .map_err(|_| stable_error("CERTIFICATE_PARSE_FAILED"))
    }

    fn client_identity(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
        alias: Option<&str>,
    ) -> AppResult<Option<SocketTlsIdentity>> {
        let Some(alias) = alias else {
            return Ok(None);
        };
        let mut identity = None;
        request.visit_materials(|material| {
            if identity.is_none() && material.alias() == alias {
                identity = Some(
                    if material.kind() == EnvironmentMaterialProbeKind::Certificate
                        && material.role() == "upstream_client_identity"
                    {
                        self.parse_client_identity(material)
                    } else {
                        Err(stable_error("MATERIAL_ALIAS_TYPE_MISMATCH"))
                    },
                );
            }
        });
        identity
            .ok_or_else(|| stable_error("MATERIAL_ALIAS_MISSING"))?
            .map(Some)
    }

    fn parse_client_identity(
        &self,
        material: EnvironmentMaterialProbe<'_>,
    ) -> AppResult<SocketTlsIdentity> {
        let encoding = material
            .encoding()
            .ok_or_else(|| stable_error("CERTIFICATE_PARSE_FAILED"))?;
        let decoded = decode_certificate_material(encoding, material.content())?;
        match encoding {
            "pem" => self
                .certificates
                .parse_client_identity_pem(&decoded)
                .map(|identity| SocketTlsIdentity {
                    certificate_chain_der: identity.certificate_chain_der.clone(),
                    private_key_pkcs8_der: identity.private_key_pkcs8_der.clone(),
                }),
            "pkcs12_base64" => self
                .certificates
                .parse_pkcs12(&decoded, material.password().unwrap_or_default())
                .map(|identity| {
                    let mut certificate_chain_der = vec![identity.certificate_der.clone()];
                    certificate_chain_der.extend(identity.chain_der.clone());
                    SocketTlsIdentity {
                        certificate_chain_der,
                        private_key_pkcs8_der: identity.private_key_pkcs8_der.clone(),
                    }
                }),
            _ => return Err(stable_error("CERTIFICATE_ROLE_MISMATCH")),
        }
        .map_err(|_| stable_error("CERTIFICATE_PARSE_FAILED"))
    }

    async fn validate_installation_roots(
        &self,
        request: &EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        let selectors = request.installation_root_selectors().collect::<Vec<_>>();
        if selectors.is_empty() {
            return Ok(EnvironmentValidationStatus::NotApplicable);
        }
        if selectors
            .iter()
            .any(|selector| *selector != INSTALLATION_ROOT_SELECTOR)
        {
            return Err(stable_error("VALIDATION_LAYER_FAILED"));
        }
        self.installation_tls
            .freeze_installation_tls_material()
            .await
            .map_err(|_| stable_error("CERTIFICATE_PARSE_FAILED"))?;
        Ok(EnvironmentValidationStatus::Passed)
    }
}

struct TlsProbeInput {
    host: String,
    port: u16,
    server_name: Option<String>,
    server_trust_der: Vec<Vec<u8>>,
    client_identity: Option<SocketTlsIdentity>,
    verify_hostname: bool,
}

fn build_tls_probe(input: TlsProbeInput) -> AppResult<SocketRelayService> {
    if input.verify_hostname
        && input
            .server_name
            .as_ref()
            .is_none_or(|server_name| server_name.parse::<std::net::IpAddr>().is_ok())
    {
        return Err(stable_error("VALIDATION_LAYER_FAILED"));
    }
    SocketRelayService::build(SocketRelayConfig {
        bind_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
        upstream: SocketEndpoint {
            host: input.host,
            port: input.port,
        },
        security: SocketRelaySecurity::TcpToTls {
            upstream_tls: SocketUpstreamTlsConfig {
                server_trust_der: input.server_trust_der,
                client_identity: input.client_identity,
                verify_hostname: input.verify_hostname,
                tls_server_name: input.server_name,
            },
        },
        maximum_connections: 1,
        read_chunk_bytes: 1,
        connect_timeout: Duration::from_secs(10),
        read_timeout: Duration::from_secs(10),
        write_timeout: Duration::from_secs(10),
    })
    .map_err(|_| stable_error("VALIDATION_LAYER_FAILED"))
}

#[async_trait]
impl EnvironmentValidationLayerPort for EnvironmentConfigurationValidationAdapter {
    async fn validate_layer(
        &self,
        request: EnvironmentValidationLayerRequest<'_>,
    ) -> AppResult<EnvironmentValidationStatus> {
        match request.layer() {
            EnvironmentValidationLayer::Schema | EnvironmentValidationLayer::Domain => {
                Ok(EnvironmentValidationStatus::Passed)
            }
            EnvironmentValidationLayer::Material => self.validate_materials(&request),
            EnvironmentValidationLayer::PackageProjection => self.validate_packages(&request).await,
            EnvironmentValidationLayer::DnsTcpPort => self.validate_dns_tcp(&request).await,
            EnvironmentValidationLayer::TlsMtls => self.validate_tls_mtls(&request).await,
            EnvironmentValidationLayer::PreviewBaseline => Ok(EnvironmentValidationStatus::Passed),
        }
    }
}

fn decode_certificate_material(encoding: &str, content: &str) -> AppResult<Zeroizing<Vec<u8>>> {
    match encoding {
        "pem" => Ok(Zeroizing::new(content.as_bytes().to_vec())),
        "base64_der" | "pkcs12_base64" => STANDARD
            .decode(content)
            .map(Zeroizing::new)
            .map_err(|_| stable_error("CERTIFICATE_PARSE_FAILED")),
        _ => Err(stable_error("CERTIFICATE_ROLE_MISMATCH")),
    }
}

fn stable_error(code: &'static str) -> AppError {
    AppError::new(code, "环境配置技术验证失败。")
}

#[cfg(test)]
#[path = "environment_configuration_validation_tests.rs"]
mod tests;
