//! 通用代理 Workspace 领域模型。
//!
//! Workspace 是桌面 UI、未来 TUI/CLI 和无界面测试共同使用的配置边界。这里仅保存
//! 可序列化配置与安全引用，不保存证书私钥、PKCS#12 密码、代理认证明文或文件内容。
//! 因此 `.intercept-workspace` 可以安全地经过统一导入导出流程，但真正的秘密仍由
//! infrastructure 根据 [`SecretReference`] 从系统密钥库中解析。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AndroidNetworkProfile, CertificateReferenceId, DomainError, ErrorCode, FaultPresetId,
    ListenerId, MetadataExtractorId, ResponseAssertionId, Revision, Rule, RuleAction, WorkspaceId,
};

/// 首次启动创建的正向代理草稿端口。监听器默认禁用，因此不会在用户确认前打开端口。
pub const DEFAULT_FORWARD_PROXY_PORT: u16 = 8080;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 系统密钥库中一项秘密的稳定引用。
/// `provider` 例如 `keychain`、`dpapi` 或测试内存实现；`key` 是该 provider 内的标识。
/// 结构中刻意没有 `value`、`password` 或私钥字节字段。
pub struct SecretReference {
    pub provider: String,
    pub key: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BodyCodecKind {
    #[default]
    Raw,
    Utf8,
    ShiftJis,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetadataExtractorSource {
    Header { name: String },
    JsonPath { path: String },
    BodyText,
    FixedValue { value: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 从通用 HTTP 报文提取列表/规则所需的少量元数据。
pub struct MetadataExtractor {
    pub id: MetadataExtractorId,
    pub name: String,
    pub listener_ids: Vec<ListenerId>,
    pub source: MetadataExtractorSource,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseAssertionKind {
    HttpStatusEquals {
        expected: u16,
    },
    HeaderEquals {
        name: String,
        expected: String,
    },
    JsonPathEquals {
        path: String,
        #[specta(type = specta_typescript::Unknown<Value>)]
        expected: Value,
    },
    BodyTextContains {
        expected: String,
    },
    BodyLengthEquals {
        expected: u64,
    },
    BodySha256Equals {
        expected_hex: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 用户可配置的响应断言。核心只比较通用 HTTP 数据，不包含任何业务返回码。
pub struct ResponseAssertion {
    pub id: ResponseAssertionId,
    pub name: String,
    pub listener_ids: Vec<ListenerId>,
    pub enabled: bool,
    pub assertion: ResponseAssertionKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CertificateReferenceKind {
    MitmRootCa,
    ReverseServerIdentity,
    DownstreamClientTrust,
    UpstreamClientIdentity,
    UpstreamServerTrust,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 证书材料的非敏感引用。实际证书链和私钥由 infrastructure 解析。
pub struct CertificateReference {
    pub id: CertificateReferenceId,
    pub label: String,
    pub kind: CertificateReferenceKind,
    pub reference: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ForwardProxyAuthentication {
    None,
    Basic { credential: SecretReference },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct MitmSettings {
    pub enabled: bool,
    /// 精确主机名或 `*.example.test` 形式的单层/多层子域后缀。
    pub authority_allowlist: Vec<String>,
    pub root_ca: Option<CertificateReferenceId>,
    pub maximum_cached_leaf_certificates: u16,
}

impl Default for MitmSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            authority_allowlist: Vec::new(),
            root_ca: None,
            maximum_cached_leaf_certificates: 256,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum DownstreamClientAuthentication {
    Disabled,
    Optional { trust: CertificateReferenceId },
    Required { trust: CertificateReferenceId },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct DownstreamTlsSettings {
    pub enabled: bool,
    /// `None` 表示使用证书管理页的 Root CA，按客户端 SNI 动态签发服务端证书。
    /// `Some` 仅用于显式选择 Workspace 内的固定服务端身份引用。
    pub server_identity: Option<CertificateReferenceId>,
    /// 允许动态签发的精确 DNS/IP 或 `*.example.test` 域名模式。Android 透明代理路由
    /// 目标与固定 Server 主机名会在运行时自动合并到此列表，
    /// 因此这里只需填写额外允许的客户端访问域名。
    #[serde(default)]
    pub dynamic_sni_allowlist: Vec<String>,
    pub client_authentication: DownstreamClientAuthentication,
}

impl Default for DownstreamTlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            server_identity: None,
            dynamic_sni_allowlist: Vec::new(),
            client_authentication: DownstreamClientAuthentication::Disabled,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct UpstreamTlsSettings {
    pub verify_hostname: bool,
    /// `None` 表示使用操作系统信任根。
    pub server_trust: Option<CertificateReferenceId>,
    pub client_identity: Option<CertificateReferenceId>,
}

impl Default for UpstreamTlsSettings {
    fn default() -> Self {
        Self {
            verify_hostname: true,
            server_trust: None,
            client_identity: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 将该监听器收到的全部 HTTP 请求转发到一个固定 Server。
/// `None` 表示普通正向代理：每个请求使用自己的目标地址。`Some` 表示固定 Server
/// 模式：监听器仍是同一条代理入口，只是目的地改由这里统一指定。上游 CA 和可选的
/// mTLS 客户端身份属于这条固定转发配置，不能放在全局证书页或另一条“上游入口”中。
pub struct FixedServerSettings {
    /// 固定上游 origin，只允许 `http`/`https`、主机和可选端口。
    pub upstream_url: String,
    pub upstream_tls: UpstreamTlsSettings,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Type)]
/// 用户面对的唯一代理监听配置。
/// 不再用 `Forward`/`Reverse` 两种公开类型割裂配置流程。监听地址、访问控制、下游
/// TLS 和超时始终属于监听器；是否固定转发到 Server 只由 [`Self::fixed_server`] 决定。
pub struct ProxyListener {
    pub id: ListenerId,
    pub name: String,
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub authentication: ForwardProxyAuthentication,
    pub allowed_client_cidrs: Vec<String>,
    pub mitm: MitmSettings,
    pub connect_timeout_ms: u64,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub downstream_tls: DownstreamTlsSettings,
    /// 当前监听处理请求正文时采用的字符编码。Raw 表示不执行文本/JSON解码。
    pub request_body_codec: BodyCodecKind,
    /// 当前监听处理响应正文时采用的字符编码。Raw 表示不执行文本/JSON解码。
    pub response_body_codec: BodyCodecKind,
    pub fixed_server: Option<FixedServerSettings>,
}

#[derive(Deserialize)]
struct ProxyListenerDocument {
    id: ListenerId,
    name: String,
    enabled: bool,
    bind_address: String,
    port: u16,
    authentication: ForwardProxyAuthentication,
    allowed_client_cidrs: Vec<String>,
    mitm: MitmSettings,
    connect_timeout_ms: u64,
    read_timeout_ms: u64,
    write_timeout_ms: u64,
    downstream_tls: Option<DownstreamTlsSettings>,
    #[serde(default)]
    request_body_codec: BodyCodecKind,
    #[serde(default)]
    response_body_codec: BodyCodecKind,
    fixed_server: Option<FixedServerSettings>,
}

impl<'de> Deserialize<'de> for ProxyListener {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let document = ProxyListenerDocument::deserialize(deserializer)?;
        Ok(Self {
            id: document.id,
            name: document.name,
            enabled: document.enabled,
            bind_address: document.bind_address,
            port: document.port,
            authentication: document.authentication,
            allowed_client_cidrs: document.allowed_client_cidrs,
            mitm: document.mitm,
            connect_timeout_ms: document.connect_timeout_ms,
            read_timeout_ms: document.read_timeout_ms,
            write_timeout_ms: document.write_timeout_ms,
            downstream_tls: document.downstream_tls.unwrap_or_default(),
            request_body_codec: document.request_body_codec,
            response_body_codec: document.response_body_codec,
            fixed_server: document.fixed_server,
        })
    }
}

impl Default for ProxyListener {
    fn default() -> Self {
        Self {
            id: ListenerId::new(),
            name: "默认代理监听".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: DEFAULT_FORWARD_PROXY_PORT,
            authentication: ForwardProxyAuthentication::None,
            allowed_client_cidrs: Vec::new(),
            mitm: MitmSettings::default(),
            connect_timeout_ms: 30_000,
            read_timeout_ms: 70_000,
            write_timeout_ms: 70_000,
            downstream_tls: DownstreamTlsSettings::default(),
            request_body_codec: BodyCodecKind::Raw,
            response_body_codec: BodyCodecKind::Raw,
            fixed_server: None,
        }
    }
}

impl ProxyListener {
    #[must_use]
    pub const fn id(&self) -> ListenerId {
        self.id
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub fn bind_endpoint(&self) -> (&str, u16) {
        (&self.bind_address, self.port)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionFaultAction {
    Delay { milliseconds: u64 },
    Reject,
    RateLimit { bytes_per_second: u64 },
    CloseAfterBytes { bytes: u64 },
    HalfCloseAfterBytes { bytes: u64 },
    IdleTimeout { milliseconds: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct FaultPreset {
    pub id: FaultPresetId,
    pub name: String,
    pub description: String,
    pub connection_actions: Vec<ConnectionFaultAction>,
    /// 规则编辑使用独立、已生成 TypeScript 的 Rule DTO；Workspace 编辑页不直接修改
    /// 动作联合类型，因此这里在 Specta Workspace DTO 中省略，Serde 持久化仍完整保留。
    #[specta(skip)]
    pub http_actions: Vec<RuleAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
pub struct ProxyWorkspace {
    pub id: WorkspaceId,
    pub name: String,
    pub revision: Revision,
    pub listeners: Vec<ProxyListener>,
    pub metadata_extractors: Vec<MetadataExtractor>,
    pub response_assertions: Vec<ResponseAssertion>,
    /// 规则通过 rule_* 用例维护，避免前端在 Workspace 表单中复制第二套规则编辑器。
    /// 字段仍属于领域聚合并参与导入导出，只不重复进入 Workspace 的 TypeScript DTO。
    #[specta(skip)]
    pub rules: Vec<Rule>,
    pub fault_presets: Vec<FaultPreset>,
    pub certificate_references: Vec<CertificateReference>,
    /// 与该 Workspace 一起迁移的 Android 设备网络方案。
    /// 设备序列号、ADB transport、已解析桌面地址和运行态由宿主在启动时提供，不属于此字段。
    #[serde(default)]
    pub android_network_profiles: Vec<AndroidNetworkProfile>,
}

impl Default for ProxyWorkspace {
    fn default() -> Self {
        Self {
            id: WorkspaceId::new(),
            name: "Untitled Workspace".into(),
            revision: Revision::INITIAL,
            listeners: vec![ProxyListener::default()],
            metadata_extractors: Vec::new(),
            response_assertions: Vec::new(),
            rules: Vec::new(),
            fault_presets: Vec::new(),
            certificate_references: Vec::new(),
            android_network_profiles: Vec::new(),
        }
    }
}

impl ProxyWorkspace {
    /// 聚合全部字段错误，保证任何 Host 都得到相同校验结果。
    pub fn validate(&self) -> Result<(), DomainError> {
        let mut error = DomainError::new(ErrorCode::ConfigInvalid, "Workspace 配置存在字段错误");
        if self.name.trim().is_empty() {
            push_field_error(&mut error, "name", "Workspace 名称不能为空");
        }

        let certificate_ids = unique_ids(
            self.certificate_references.iter().map(|item| item.id),
            "certificate_references",
            &mut error,
        );
        let certificate_kinds = self
            .certificate_references
            .iter()
            .map(|item| (item.id, item.kind))
            .collect::<BTreeMap<_, _>>();
        for (index, reference) in self.certificate_references.iter().enumerate() {
            if reference.label.trim().is_empty() || reference.reference.trim().is_empty() {
                push_field_error(
                    &mut error,
                    format!("certificate_references.{index}"),
                    "证书名称和安全引用不能为空",
                );
            }
        }

        let listener_ids = unique_ids(
            self.listeners.iter().map(|listener| listener.id),
            "listeners",
            &mut error,
        );
        let mut enabled_endpoints = BTreeMap::new();
        for (index, listener) in self.listeners.iter().enumerate() {
            validate_listener(
                listener,
                index,
                &certificate_ids,
                &certificate_kinds,
                &mut error,
            );
            if listener.enabled {
                let endpoint = listener.bind_endpoint();
                if let Some(existing) = enabled_endpoints.insert(endpoint, index) {
                    push_field_error(
                        &mut error,
                        format!("listeners.{index}.port"),
                        format!("监听地址与 listeners.{existing} 重复"),
                    );
                }
            }
        }

        validate_workspace_references(self, &listener_ids, &mut error);

        if error.field_errors.is_empty() {
            Ok(())
        } else {
            Err(error)
        }
    }

    /// 乐观锁更新，校验失败时不会改变当前 Workspace。
    pub fn apply(
        &mut self,
        expected_revision: Revision,
        mut values: Self,
    ) -> Result<Revision, DomainError> {
        self.revision.verify(expected_revision)?;
        values.validate()?;
        let revision = self.revision.next();
        values.id = self.id;
        values.revision = revision;
        *self = values;
        Ok(revision)
    }
}

fn validate_workspace_references(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    unique_ids(
        workspace.metadata_extractors.iter().map(|item| item.id),
        "metadata_extractors",
        error,
    );
    for (index, extractor) in workspace.metadata_extractors.iter().enumerate() {
        validate_named_listener_refs(
            &extractor.name,
            &extractor.listener_ids,
            listener_ids,
            &format!("metadata_extractors.{index}"),
            error,
        );
    }

    unique_ids(
        workspace.response_assertions.iter().map(|item| item.id),
        "response_assertions",
        error,
    );
    for (index, assertion) in workspace.response_assertions.iter().enumerate() {
        validate_named_listener_refs(
            &assertion.name,
            &assertion.listener_ids,
            listener_ids,
            &format!("response_assertions.{index}"),
            error,
        );
        validate_assertion(assertion, index, error);
    }

    unique_ids(
        workspace.fault_presets.iter().map(|item| item.id),
        "fault_presets",
        error,
    );
    unique_ids(workspace.rules.iter().map(|item| item.id), "rules", error);
    for (index, rule) in workspace.rules.iter().enumerate() {
        if let Some(channel) = &rule.channel
            && !listener_ids
                .iter()
                .any(|listener_id| listener_id.to_string() == channel.as_str())
        {
            push_field_error(
                error,
                format!("rules.{index}.channel"),
                "规则通道必须引用当前 Workspace 中存在的代理入口",
            );
        }
    }

    validate_android_profiles(workspace, listener_ids, error);
}

fn validate_android_profiles(
    workspace: &ProxyWorkspace,
    listener_ids: &BTreeSet<ListenerId>,
    error: &mut DomainError,
) {
    let mut profile_ids = BTreeSet::new();
    for (index, profile) in workspace.android_network_profiles.iter().enumerate() {
        if !profile_ids.insert(profile.id.as_str()) {
            push_field_error(
                error,
                format!("android_network_profiles.{index}.id"),
                "设备网络方案 ID 不能重复",
            );
        }
        if let Err(profile_error) = profile.validate() {
            for (field, messages) in profile_error.field_errors.iter() {
                for message in messages {
                    push_field_error(
                        error,
                        format!("android_network_profiles.{index}.{field}"),
                        message.clone(),
                    );
                }
            }
        }
        for (route_index, route) in profile.proxy_routes.iter().enumerate() {
            if !listener_ids.contains(&route.listener_id) {
                push_field_error(
                    error,
                    format!(
                        "android_network_profiles.{index}.proxy_routes.{route_index}.listener_id"
                    ),
                    "透明代理路由必须引用当前 Workspace 中存在的代理入口",
                );
            }
        }
    }
}

fn validate_listener(
    listener: &ProxyListener,
    index: usize,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    error: &mut DomainError,
) {
    let prefix = format!("listeners.{index}");
    if listener.name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "监听器名称不能为空");
    }
    let bind_ip = listener.bind_address.parse::<IpAddr>();
    if bind_ip.is_err() {
        push_field_error(
            error,
            format!("{prefix}.bind_address"),
            "绑定地址必须是有效 IP",
        );
    }
    if listener.port == 0 {
        push_field_error(error, format!("{prefix}.port"), "监听端口必须大于 0");
    }

    validate_listener_access(
        listener,
        bind_ip.ok(),
        certificate_ids,
        certificate_kinds,
        &prefix,
        error,
    );
    validate_downstream_tls(listener, certificate_ids, certificate_kinds, &prefix, error);
    if let Some(fixed_server) = &listener.fixed_server {
        validate_fixed_server(
            fixed_server,
            certificate_ids,
            certificate_kinds,
            &prefix,
            error,
        );
    }
}

fn validate_listener_access(
    value: &ProxyListener,
    bind_ip: Option<IpAddr>,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    if value.connect_timeout_ms == 0 || value.read_timeout_ms == 0 || value.write_timeout_ms == 0 {
        push_field_error(error, format!("{prefix}.timeouts"), "超时必须大于 0 毫秒");
    }
    for (cidr_index, cidr) in value.allowed_client_cidrs.iter().enumerate() {
        if !is_valid_cidr(cidr) {
            push_field_error(
                error,
                format!("{prefix}.allowed_client_cidrs.{cidr_index}"),
                "必须是有效 IPv4/IPv6 CIDR",
            );
        }
    }
    // 固定 Server 入口常用于 App -> 本机代理的受控测试链路，并可通过下游 TLS/mTLS
    // 验证客户端。动态正向代理若暴露到非回环网络则必须额外配置认证与 CIDR 白名单，
    // 防止默认配置意外成为开放代理。
    if value.fixed_server.is_none() && bind_ip.is_some_and(|ip| !ip.is_loopback()) {
        if matches!(value.authentication, ForwardProxyAuthentication::None) {
            push_field_error(
                error,
                format!("{prefix}.authentication"),
                "非回环监听必须启用代理认证",
            );
        }
        if value.allowed_client_cidrs.is_empty() {
            push_field_error(
                error,
                format!("{prefix}.allowed_client_cidrs"),
                "非回环监听必须配置客户端 CIDR 白名单",
            );
        }
    }
    if let ForwardProxyAuthentication::Basic { credential } = &value.authentication
        && (credential.provider.trim().is_empty() || credential.key.trim().is_empty())
    {
        push_field_error(
            error,
            format!("{prefix}.authentication.credential"),
            "认证秘密引用不能为空",
        );
    }
    if value.mitm.enabled {
        if value.mitm.authority_allowlist.is_empty() {
            push_field_error(
                error,
                format!("{prefix}.mitm.authority_allowlist"),
                "启用 MITM 时必须配置显式允许列表",
            );
        }
        if value.mitm.maximum_cached_leaf_certificates == 0
            || value.mitm.maximum_cached_leaf_certificates > 256
        {
            push_field_error(
                error,
                format!("{prefix}.mitm.maximum_cached_leaf_certificates"),
                "MITM 叶子证书缓存必须在 1..=256",
            );
        }
        // `None` 表示使用当前安装实例首次启动时生成并受系统密钥保护的 Root CA。
        // 只有用户显式提供 Workspace 证书引用时才校验该引用，避免为了使用默认安装级
        // Root 而伪造文件路径或把私钥材料写入 Workspace。
        if value
            .mitm
            .root_ca
            .is_some_and(|id| !certificate_ids.contains(&id))
        {
            push_field_error(
                error,
                format!("{prefix}.mitm.root_ca"),
                "MITM Root CA 引用不存在；留空可使用当前安装实例 Root CA",
            );
        } else {
            validate_certificate_role(
                value.mitm.root_ca,
                CertificateReferenceKind::MitmRootCa,
                certificate_kinds,
                format!("{prefix}.mitm.root_ca"),
                "MITM Root CA 引用类型不匹配",
                error,
            );
        }
        for (allow_index, authority) in value.mitm.authority_allowlist.iter().enumerate() {
            if !is_valid_authority_pattern(authority) {
                push_field_error(
                    error,
                    format!("{prefix}.mitm.authority_allowlist.{allow_index}"),
                    "必须是精确 DNS/IP 或 *.example.test 形式",
                );
            }
        }
    }
}

fn validate_downstream_tls(
    value: &ProxyListener,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    // `None` 明确表示使用当前安装实例的 Root CA 按客户端 SNI 动态签发叶子证书。
    // 只有用户改选固定 Workspace 身份时才要求该引用存在。
    if value.downstream_tls.enabled
        && value
            .downstream_tls
            .server_identity
            .is_some_and(|id| !certificate_ids.contains(&id))
    {
        push_field_error(
            error,
            format!("{prefix}.downstream_tls.server_identity"),
            "下游 TLS 服务端身份引用不存在；留空可使用证书管理页签发的本机叶子证书",
        );
    } else {
        validate_certificate_role(
            value.downstream_tls.server_identity,
            CertificateReferenceKind::ReverseServerIdentity,
            certificate_kinds,
            format!("{prefix}.downstream_tls.server_identity"),
            "下游 TLS 服务端身份引用类型不匹配",
            error,
        );
    }
    for (allow_index, authority) in value
        .downstream_tls
        .dynamic_sni_allowlist
        .iter()
        .enumerate()
    {
        if !is_valid_authority_pattern(authority) {
            push_field_error(
                error,
                format!("{prefix}.downstream_tls.dynamic_sni_allowlist.{allow_index}"),
                "必须是精确 DNS/IP 或 *.example.test 形式",
            );
        }
    }
    let downstream_trust = match value.downstream_tls.client_authentication {
        DownstreamClientAuthentication::Disabled => None,
        DownstreamClientAuthentication::Optional { trust }
        | DownstreamClientAuthentication::Required { trust } => Some(trust),
    };
    if downstream_trust.is_some_and(|id| !certificate_ids.contains(&id)) {
        push_field_error(
            error,
            format!("{prefix}.downstream_tls.client_authentication"),
            "下游客户端信任引用不存在",
        );
    } else {
        validate_certificate_role(
            downstream_trust,
            CertificateReferenceKind::DownstreamClientTrust,
            certificate_kinds,
            format!("{prefix}.downstream_tls.client_authentication"),
            "下游客户端信任引用类型不匹配",
            error,
        );
    }
}

fn validate_fixed_server(
    value: &FixedServerSettings,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    prefix: &str,
    error: &mut DomainError,
) {
    let fixed_prefix = format!("{prefix}.fixed_server");
    if !is_valid_upstream_origin(&value.upstream_url) {
        push_field_error(
            error,
            format!("{fixed_prefix}.upstream_url"),
            "固定 Server 必须是 HTTP/HTTPS origin，不能包含路径、查询、片段或用户信息",
        );
    }
    let uses_https = value
        .upstream_url
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"));
    if !uses_https
        && (value.upstream_tls.server_trust.is_some()
            || value.upstream_tls.client_identity.is_some())
    {
        push_field_error(
            error,
            format!("{fixed_prefix}.upstream_tls"),
            "Server CA 和 mTLS 客户端身份只能用于 HTTPS Server",
        );
    }
    for (field, reference) in [
        ("server_trust", value.upstream_tls.server_trust),
        ("client_identity", value.upstream_tls.client_identity),
    ] {
        if reference.is_some_and(|id| !certificate_ids.contains(&id)) {
            push_field_error(
                error,
                format!("{fixed_prefix}.upstream_tls.{field}"),
                "Server TLS 证书引用不存在",
            );
        }
    }
    validate_certificate_role(
        value.upstream_tls.server_trust,
        CertificateReferenceKind::UpstreamServerTrust,
        certificate_kinds,
        format!("{fixed_prefix}.upstream_tls.server_trust"),
        "上游 Server CA 引用类型不匹配",
        error,
    );
    validate_certificate_role(
        value.upstream_tls.client_identity,
        CertificateReferenceKind::UpstreamClientIdentity,
        certificate_kinds,
        format!("{fixed_prefix}.upstream_tls.client_identity"),
        "上游 mTLS 客户端身份引用类型不匹配",
        error,
    );
}

fn validate_certificate_role(
    reference: Option<CertificateReferenceId>,
    expected: CertificateReferenceKind,
    certificate_kinds: &BTreeMap<CertificateReferenceId, CertificateReferenceKind>,
    field: String,
    message: &str,
    error: &mut DomainError,
) {
    if reference.is_some_and(|id| {
        certificate_kinds
            .get(&id)
            .is_some_and(|kind| *kind != expected)
    }) {
        push_field_error(error, field, message);
    }
}

fn validate_named_listener_refs(
    name: &str,
    listener_ids: &[ListenerId],
    existing: &BTreeSet<ListenerId>,
    prefix: &str,
    error: &mut DomainError,
) {
    if name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "名称不能为空");
    }
    for (index, id) in listener_ids.iter().enumerate() {
        if !existing.contains(id) {
            push_field_error(
                error,
                format!("{prefix}.listener_ids.{index}"),
                "引用的监听器不存在",
            );
        }
    }
}

fn validate_assertion(assertion: &ResponseAssertion, index: usize, error: &mut DomainError) {
    let prefix = format!("response_assertions.{index}.assertion");
    match &assertion.assertion {
        ResponseAssertionKind::HttpStatusEquals { expected } if !(100..=599).contains(expected) => {
            push_field_error(error, prefix, "HTTP 状态码必须在 100..=599");
        }
        ResponseAssertionKind::HeaderEquals { name, .. } if name.trim().is_empty() => {
            push_field_error(error, prefix, "Header 名称不能为空");
        }
        ResponseAssertionKind::JsonPathEquals { path, .. } if path.trim().is_empty() => {
            push_field_error(error, prefix, "JSONPath 不能为空");
        }
        ResponseAssertionKind::BodySha256Equals { expected_hex }
            if expected_hex.len() != 64
                || !expected_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
        {
            push_field_error(error, prefix, "SHA-256 必须是 64 位十六进制字符串");
        }
        _ => {}
    }
}

fn unique_ids<T: Copy + Ord>(
    values: impl Iterator<Item = T>,
    field: &str,
    error: &mut DomainError,
) -> BTreeSet<T> {
    let mut ids = BTreeSet::new();
    for (index, id) in values.enumerate() {
        if !ids.insert(id) {
            push_field_error(error, format!("{field}.{index}.id"), "ID 不能重复");
        }
    }
    ids
}

fn push_field_error(error: &mut DomainError, field: impl Into<String>, message: impl Into<String>) {
    error
        .field_errors
        .entry(field.into())
        .or_default()
        .push(message.into());
}

#[must_use]
pub fn is_valid_cidr(value: &str) -> bool {
    let Some((address, prefix)) = value.split_once('/') else {
        return false;
    };
    let Ok(address) = address.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix) = prefix.parse::<u8>() else {
        return false;
    };
    prefix <= if address.is_ipv4() { 32 } else { 128 }
}

#[must_use]
pub fn is_valid_upstream_origin(value: &str) -> bool {
    let rest = value
        .strip_prefix("http://")
        .or_else(|| value.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty()
        || authority.contains('@')
        || !matches!(&rest[authority_end..], "" | "/")
    {
        return false;
    }
    valid_authority(authority)
}

fn is_valid_authority_pattern(value: &str) -> bool {
    let value = value.strip_prefix("*.").unwrap_or(value);
    !value.is_empty() && valid_host(value)
}

fn valid_authority(authority: &str) -> bool {
    if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let suffix = &bracketed[end + 1..];
        return bracketed[..end].parse::<std::net::Ipv6Addr>().is_ok()
            && (suffix.is_empty() || suffix.strip_prefix(':').is_some_and(valid_port));
    }
    if let Some((host, port)) = authority.rsplit_once(':') {
        return !host.contains(':') && valid_host(host) && valid_port(port);
    }
    valid_host(authority)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

fn valid_host(value: &str) -> bool {
    value.parse::<IpAddr>().is_ok()
        || (!value.is_empty()
            && value.len() <= 253
            && value.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AndroidProxyRoute, AndroidTargetApplication, WeakNetworkProfile};

    #[test]
    fn default_workspace_is_empty_safe_and_serializable() {
        let workspace = ProxyWorkspace::default();
        assert_eq!(workspace.listeners.len(), 1);
        let listener = &workspace.listeners[0];
        assert!(!listener.enabled);
        assert_eq!(listener.bind_address, "127.0.0.1");
        assert_eq!(listener.port, 8080);
        assert!(listener.fixed_server.is_none());
        assert!(workspace.rules.is_empty());
        assert!(workspace.fault_presets.is_empty());
        workspace.validate().expect("safe draft must validate");
        let json = serde_json::to_string(&workspace).unwrap();
        for forbidden in ["private_key", "password", "pkcs12"] {
            assert!(!json.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn dynamic_listener_without_new_optional_downstream_tls_still_loads() {
        let mut document = serde_json::to_value(ProxyWorkspace::default()).unwrap();
        document["listeners"][0]
            .as_object_mut()
            .unwrap()
            .remove("downstream_tls");

        let workspace: ProxyWorkspace = serde_json::from_value(document).unwrap();

        assert!(!workspace.listeners[0].downstream_tls.enabled);
        assert!(workspace.listeners[0].fixed_server.is_none());
        workspace.validate().unwrap();
    }

    #[test]
    fn non_loopback_forward_listener_requires_authentication_and_cidr() {
        let mut workspace = ProxyWorkspace::default();
        let listener = &mut workspace.listeners[0];
        listener.enabled = true;
        listener.bind_address = "0.0.0.0".into();
        let error = workspace.validate().unwrap_err();
        assert!(
            error
                .field_errors
                .contains_key("listeners.0.authentication")
        );
        assert!(
            error
                .field_errors
                .contains_key("listeners.0.allowed_client_cidrs")
        );
    }

    #[test]
    fn fixed_server_accepts_generic_http_and_https_origins() {
        assert!(is_valid_upstream_origin("http://127.0.0.1:8081"));
        assert!(is_valid_upstream_origin("https://example.test:443/"));
        for invalid in [
            "ftp://example.test",
            "https://user@example.test",
            "https://example.test/path",
            "https://example.test?query=1",
        ] {
            assert!(!is_valid_upstream_origin(invalid), "{invalid}");
        }
    }

    #[test]
    fn workspace_accepts_multiple_fixed_server_listener_mappings() {
        let fixed = |name: &str, port: u16, upstream_url: &str| ProxyListener {
            id: ListenerId::new(),
            name: name.into(),
            enabled: true,
            bind_address: "127.0.0.1".into(),
            port,
            request_body_codec: BodyCodecKind::Raw,
            response_body_codec: BodyCodecKind::Raw,
            fixed_server: Some(FixedServerSettings {
                upstream_url: upstream_url.into(),
                upstream_tls: UpstreamTlsSettings::default(),
            }),
            ..ProxyListener::default()
        };
        let workspace = ProxyWorkspace {
            listeners: vec![
                fixed(
                    "Transaction",
                    16_627,
                    "https://transaction.example.test:16627",
                ),
                fixed("DLL", 16_127, "https://dll.example.test:16127"),
            ],
            ..ProxyWorkspace::default()
        };

        workspace
            .validate()
            .expect("distinct local endpoints may map to distinct upstream origins");
    }

    #[test]
    fn mitm_is_fail_closed_without_allowlist_but_can_use_installation_root() {
        let mut workspace = ProxyWorkspace::default();
        let listener = &mut workspace.listeners[0];
        listener.mitm.enabled = true;
        let error = workspace.validate().unwrap_err();
        assert!(
            error
                .field_errors
                .contains_key("listeners.0.mitm.authority_allowlist")
        );
        assert!(!error.field_errors.contains_key("listeners.0.mitm.root_ca"));
    }

    #[test]
    fn downstream_tls_can_use_installation_root_for_dynamic_sni() {
        let mut workspace = ProxyWorkspace::default();
        workspace.listeners[0].downstream_tls.enabled = true;
        workspace.listeners[0].downstream_tls.server_identity = None;
        workspace.listeners[0].downstream_tls.dynamic_sni_allowlist =
            vec!["api.example.test".into(), "*.service.test".into()];

        workspace
            .validate()
            .expect("installation root supports validated dynamic SNI patterns");
    }

    #[test]
    fn downstream_tls_rejects_invalid_dynamic_sni_pattern() {
        let mut workspace = ProxyWorkspace::default();
        workspace.listeners[0].downstream_tls.enabled = true;
        workspace.listeners[0].downstream_tls.dynamic_sni_allowlist =
            vec!["https://api.example.test/path".into()];

        let error = workspace.validate().expect_err("invalid SNI pattern");
        assert!(
            error
                .field_errors
                .contains_key("listeners.0.downstream_tls.dynamic_sni_allowlist.0")
        );
    }

    #[test]
    fn fixed_http_server_rejects_tls_certificate_configuration() {
        let mut workspace = ProxyWorkspace::default();
        let trust_id = CertificateReferenceId::new();
        workspace.certificate_references.push(CertificateReference {
            id: trust_id,
            label: "测试 Server CA".into(),
            kind: CertificateReferenceKind::UpstreamServerTrust,
            reference: "managed:test-ca".into(),
        });
        workspace.listeners[0].fixed_server = Some(FixedServerSettings {
            upstream_url: "http://server.example.test:8080".into(),
            upstream_tls: UpstreamTlsSettings {
                server_trust: Some(trust_id),
                ..UpstreamTlsSettings::default()
            },
        });

        let error = workspace.validate().unwrap_err();
        assert!(
            error
                .field_errors
                .contains_key("listeners.0.fixed_server.upstream_tls")
        );
    }

    #[test]
    fn listener_tls_rejects_certificate_references_used_in_the_wrong_role() {
        let mut workspace = ProxyWorkspace::default();
        let trust_id = CertificateReferenceId::new();
        workspace.certificate_references.push(CertificateReference {
            id: trust_id,
            label: "客户端证书 CA".into(),
            kind: CertificateReferenceKind::DownstreamClientTrust,
            reference: "managed:listener-tls:test-client-ca".into(),
        });
        workspace.listeners[0].fixed_server = Some(FixedServerSettings {
            upstream_url: "https://server.example.test:443".into(),
            upstream_tls: UpstreamTlsSettings {
                server_trust: Some(trust_id),
                ..UpstreamTlsSettings::default()
            },
        });

        let error = workspace.validate().unwrap_err();

        assert!(
            error
                .field_errors
                .contains_key("listeners.0.fixed_server.upstream_tls.server_trust")
        );
    }

    #[test]
    fn fixed_server_stores_body_encoding_on_the_listener() {
        let mut workspace = ProxyWorkspace::default();
        workspace.listeners[0].request_body_codec = BodyCodecKind::Utf8;
        workspace.listeners[0].response_body_codec = BodyCodecKind::ShiftJis;
        workspace.listeners[0].fixed_server = Some(FixedServerSettings {
            upstream_url: "https://example.test".into(),
            upstream_tls: UpstreamTlsSettings::default(),
        });

        workspace
            .validate()
            .expect("listener body codecs are self-contained");
        assert_eq!(
            workspace.listeners[0].request_body_codec,
            BodyCodecKind::Utf8
        );
        assert_eq!(
            workspace.listeners[0].response_body_codec,
            BodyCodecKind::ShiftJis
        );
    }

    #[test]
    fn android_proxy_routes_must_reference_a_listener_in_the_same_workspace() {
        let mut workspace = ProxyWorkspace::default();
        let profile = AndroidNetworkProfile {
            id: "android-route".into(),
            name: "Android route".into(),
            target_applications: vec![AndroidTargetApplication {
                package_name: "com.example.client".into(),
                uid: 10_001,
                display_name: None,
            }],
            destination_targets: Vec::new(),
            proxy_routes: vec![AndroidProxyRoute {
                destination: "api.example.test".into(),
                ports: vec![443],
                listener_id: workspace.listeners[0].id,
            }],
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        };
        workspace.android_network_profiles.push(profile);
        workspace.validate().expect("same-workspace listener route");

        workspace.android_network_profiles[0].proxy_routes[0].listener_id = ListenerId::new();
        let error = workspace.validate().expect_err("dangling listener route");
        assert!(
            error
                .field_errors
                .contains_key("android_network_profiles.0.proxy_routes.0.listener_id")
        );
    }

    #[test]
    fn apply_is_atomic_and_preserves_workspace_identity() {
        let mut stored = ProxyWorkspace::default();
        let original_id = stored.id;
        let mut candidate = stored.clone();
        candidate.id = WorkspaceId::new();
        candidate.name = "Renamed".into();
        let revision = stored.apply(Revision::INITIAL, candidate).unwrap();
        assert_eq!(revision, Revision::new(2));
        assert_eq!(stored.id, original_id);
        assert_eq!(stored.name, "Renamed");
    }

    #[test]
    fn cidr_validation_handles_ipv4_and_ipv6() {
        assert!(is_valid_cidr("127.0.0.0/8"));
        assert!(is_valid_cidr("2001:db8::/32"));
        assert!(!is_valid_cidr("10.0.0.0/33"));
        assert!(!is_valid_cidr("not-an-ip/24"));
    }
}
