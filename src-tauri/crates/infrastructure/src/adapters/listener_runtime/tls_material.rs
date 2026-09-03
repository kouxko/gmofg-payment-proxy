//! TLS 证书引用解析与运行时配置装配。

use super::{
    AppError, AppResult, BTreeSet, CertificateReference, CertificateReferenceId,
    DownstreamClientAuthentication, FixedServerSettings, IpAddr, ListenerRuntimeAdapter,
    ProxyListener, ProxyWorkspace, ReverseClientIdentity, ReverseDownstreamTls, ReverseUpstreamTls,
    normalize_android_network_destination,
};
use intercept_proxy_domain::HttpListenerSettings;

impl ListenerRuntimeAdapter {
    pub(super) async fn downstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
        http: &HttpListenerSettings,
    ) -> AppResult<Option<ReverseDownstreamTls>> {
        if !http.downstream_tls.enabled {
            return Ok(None);
        }

        let dynamic_sni = http.downstream_tls.server_identity.is_none();
        let installation_material = if dynamic_sni {
            Some(
                self.mitm_certificate_authority
                    .clone()
                    .ok_or_else(|| {
                        AppError::new(
                            "CERTIFICATE_NOT_READY",
                            "动态 SNI 服务端证书已启用，但安装级 Root CA 签发能力尚未就绪。",
                        )
                        .entity(listener.id.to_string())
                    })?
                    .freeze_installation_tls_material()
                    .await?,
            )
        } else {
            None
        };
        let server_identity = match http.downstream_tls.server_identity {
            Some(identity_id) => {
                self.load_identity(certificate_reference(workspace, identity_id)?)
                    .await?
            }
            None => installation_material
                .as_ref()
                .expect("dynamic SNI material was loaded above")
                .server_identity
                .clone(),
        };
        let (client_trust_der, client_authentication_required) =
            match http.downstream_tls.client_authentication {
                DownstreamClientAuthentication::Disabled => (Vec::new(), false),
                DownstreamClientAuthentication::Optional { trust } => (
                    self.load_trust(certificate_reference(workspace, trust)?)
                        .await?,
                    false,
                ),
                DownstreamClientAuthentication::Required { trust } => (
                    self.load_trust(certificate_reference(workspace, trust)?)
                        .await?,
                    true,
                ),
            };
        let (dynamic_server_identity, dynamic_server_name_allowlist) = if dynamic_sni {
            let allowlist = downstream_sni_allowlist(workspace, listener, http);
            if allowlist.is_empty() {
                return Err(AppError::new(
                    "DYNAMIC_SNI_ALLOWLIST_EMPTY",
                    "动态 SNI 服务端证书已启用，但当前监听没有可签发的允许域名。",
                )
                .entity(listener.id.to_string()));
            }
            (
                Some(
                    installation_material
                        .expect("dynamic SNI material was loaded above")
                        .dynamic_authority,
                ),
                allowlist,
            )
        } else {
            (None, Vec::new())
        };
        Ok(Some(ReverseDownstreamTls {
            server_identity,
            dynamic_server_identity,
            dynamic_server_name_allowlist,
            client_trust_der,
            client_authentication_required,
        }))
    }

    pub(super) async fn upstream_tls(
        &self,
        workspace: &ProxyWorkspace,
        fixed_server: &FixedServerSettings,
    ) -> AppResult<Option<ReverseUpstreamTls>> {
        if !fixed_server.upstream_url.starts_with("https://") {
            return Ok(None);
        }

        let server_trust_der = match fixed_server.upstream_tls.server_trust {
            Some(id) => {
                self.load_trust(certificate_reference(workspace, id)?)
                    .await?
            }
            None => Vec::new(),
        };
        let client_identity = match fixed_server.upstream_tls.client_identity {
            Some(id) => Some(
                self.load_identity(certificate_reference(workspace, id)?)
                    .await?,
            ),
            None => None,
        };
        Ok(Some(ReverseUpstreamTls {
            server_trust_der,
            client_identity,
            verify_hostname: fixed_server.upstream_tls.verify_hostname,
        }))
    }

    pub(super) async fn load_trust(
        &self,
        reference: &CertificateReference,
    ) -> AppResult<Vec<Vec<u8>>> {
        if let Some(resolver) = self.managed_listener_certificates.as_ref()
            && let Some(result) = resolver.resolve_trust(reference).await
        {
            return result;
        }
        Err(unmanaged_certificate_reference(reference))
    }

    pub(super) async fn load_identity(
        &self,
        reference: &CertificateReference,
    ) -> AppResult<ReverseClientIdentity> {
        if let Some(resolver) = self.managed_listener_certificates.as_ref()
            && let Some(result) = resolver.resolve_identity(reference).await
        {
            return result;
        }
        Err(unmanaged_certificate_reference(reference))
    }

    pub(super) async fn load_trust_by_id(
        &self,
        workspace: &ProxyWorkspace,
        id: CertificateReferenceId,
    ) -> AppResult<Vec<Vec<u8>>> {
        self.load_trust(certificate_reference(workspace, id)?).await
    }

    pub(super) async fn load_identity_by_id(
        &self,
        workspace: &ProxyWorkspace,
        id: CertificateReferenceId,
    ) -> AppResult<ReverseClientIdentity> {
        self.load_identity(certificate_reference(workspace, id)?)
            .await
    }
}

fn unmanaged_certificate_reference(reference: &CertificateReference) -> AppError {
    AppError::new(
        "LISTENER_CERTIFICATE_REFERENCE_UNTRUSTED",
        "监听运行时只接受应用受保护存储中的证书引用。请在代理入口中重新导入证书。",
    )
    .entity(reference.id.to_string())
}

/// 汇总当前监听允许动态签发的 SNI。
///
/// 显式配置用于普通客户端；固定 Server 主机名和 Android 透明路由目标用于让常见
/// 反向代理场景无需重复录入。CIDR 不是合法 SNI，因而不会进入允许列表。
fn downstream_sni_allowlist(
    workspace: &ProxyWorkspace,
    listener: &ProxyListener,
    http: &HttpListenerSettings,
) -> Vec<String> {
    let mut names = BTreeSet::new();
    names.extend(
        http.downstream_tls
            .dynamic_sni_allowlist
            .iter()
            .filter_map(|name| normalize_sni_pattern(name)),
    );
    if let Some(fixed_server) = http.fixed_server()
        && let Ok(uri) = fixed_server.upstream_url.parse::<http::Uri>()
        && let Some(host) = uri.host()
        && let Some(host) = normalize_sni_pattern(host.trim_matches(['[', ']']))
    {
        names.insert(host);
    }
    for route in workspace
        .android_network_profiles
        .iter()
        .flat_map(|profile| &profile.proxy_routes)
        .filter(|route| route.listener_id == listener.id)
    {
        if let Some(destination) = normalize_android_network_destination(&route.destination)
            && !destination.contains('/')
            && let Some(destination) = normalize_sni_pattern(&destination)
        {
            names.insert(destination);
        }
    }
    names.into_iter().collect()
}

pub(super) fn normalize_sni_pattern(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    let dns_name = value.strip_prefix("*.").unwrap_or(&value);
    (!dns_name.is_empty() && dns_name.parse::<IpAddr>().is_err()).then_some(value)
}

fn certificate_reference(
    workspace: &ProxyWorkspace,
    id: CertificateReferenceId,
) -> AppResult<&CertificateReference> {
    workspace
        .certificate_references
        .iter()
        .find(|reference| reference.id == id)
        .ok_or_else(|| {
            AppError::new("CERTIFICATE_NOT_READY", "证书安全引用不存在。").entity(id.to_string())
        })
}
