use super::{Arc, Duration, MitmCertificateAuthority, SocketAddr, Zeroizing};

#[derive(Clone)]
pub struct ReverseClientIdentity {
    pub certificate_chain_der: Vec<Vec<u8>>,
    pub private_key_pkcs8_der: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for ReverseClientIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReverseClientIdentity")
            .field("certificate_count", &self.certificate_chain_der.len())
            .field("private_key_pkcs8_der", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ReverseDownstreamTls {
    pub server_identity: ReverseClientIdentity,
    /// 当监听未绑定独立服务端身份时，按客户端 TLS SNI 动态签发匹配的叶子证书。
    ///
    /// 无 SNI 的客户端仍使用 `server_identity` 作为回退。显式导入独立身份时该字段为
    /// `None`，确保 Workspace 选择的证书不会被动态签发覆盖。
    pub dynamic_server_identity: Option<Arc<dyn MitmCertificateAuthority>>,
    /// 动态签发只允许命中此列表的 SNI，避免代理为任意主机名签发证书。
    pub dynamic_server_name_allowlist: Vec<String>,
    pub client_trust_der: Vec<Vec<u8>>,
    pub client_authentication_required: bool,
}

#[derive(Clone, Debug)]
pub struct ReverseUpstreamTls {
    pub server_trust_der: Vec<Vec<u8>>,
    pub client_identity: Option<ReverseClientIdentity>,
    pub verify_hostname: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// 使用反向代理实际运行配置完成一次上游 TLS 握手后得到的公开元数据。
///
/// 这里刻意不暴露证书 DER、客户端证书或私钥。调用者只能确认网络可达、证书链与
/// 主机名策略是否通过，以及最终协商出的 TLS 参数。
pub struct UpstreamTlsHandshakeResult {
    pub resolved_address: SocketAddr,
    pub tls_version: String,
    pub cipher_suite: String,
    pub peer_subject: String,
    pub peer_sha256_fingerprint: String,
    pub hostname_verification_enabled: bool,
    pub client_identity_configured: bool,
    pub elapsed_millis: u64,
}

#[derive(Clone, Debug)]
pub struct ReverseProxyConfig {
    pub bind_addr: SocketAddr,
    /// 允许连接当前监听的客户端网络。回环监听可留空；非回环监听必须显式配置。
    pub allowed_client_cidrs: Vec<String>,
    pub upstream_origin: String,
    pub downstream_tls: Option<ReverseDownstreamTls>,
    pub upstream_tls: Option<ReverseUpstreamTls>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
}
