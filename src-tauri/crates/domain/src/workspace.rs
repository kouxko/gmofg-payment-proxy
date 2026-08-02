//! 通用代理 Workspace 领域模型。
//!
//! Workspace 是桌面 UI、未来 TUI/CLI 和无界面测试共同使用的配置边界。这里仅保存
//! 可序列化配置与安全引用，不保存证书私钥、PKCS#12 密码、代理认证明文或文件内容。
//! 因此 `.intercept-workspace` 可以安全地经过统一导入导出流程，但真正的秘密仍由
//! infrastructure 根据 [`SecretReference`] 从系统密钥库中解析。

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    CertificateReferenceId, CodecPolicyId, DomainError, ErrorCode, FaultPresetId, ListenerId,
    MetadataExtractorId, ResponseAssertionId, Revision, Rule, RuleAction, WorkspaceId,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BodyCodecKind {
    Raw,
    Utf8,
    ShiftJis,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BodyDirection {
    Request,
    Response,
    Both,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
/// 将指定监听器、方向与 Body 编解码方式关联起来。
pub struct BodyCodecPolicy {
    pub id: CodecPolicyId,
    pub name: String,
    pub listener_ids: Vec<ListenerId>,
    pub direction: BodyDirection,
    pub codec: BodyCodecKind,
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
pub struct ForwardProxyListener {
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
}

impl Default for ForwardProxyListener {
    fn default() -> Self {
        Self {
            id: ListenerId::new(),
            name: "默认正向代理".into(),
            enabled: false,
            bind_address: "127.0.0.1".into(),
            port: DEFAULT_FORWARD_PROXY_PORT,
            authentication: ForwardProxyAuthentication::None,
            allowed_client_cidrs: Vec::new(),
            mitm: MitmSettings::default(),
            connect_timeout_ms: 30_000,
            read_timeout_ms: 70_000,
            write_timeout_ms: 70_000,
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
    pub server_identity: Option<CertificateReferenceId>,
    pub client_authentication: DownstreamClientAuthentication,
}

impl Default for DownstreamTlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            server_identity: None,
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
pub struct ReverseProxyListener {
    pub id: ListenerId,
    pub name: String,
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    /// 固定上游 origin，只允许 `http`/`https`、主机和可选端口。
    pub upstream_url: String,
    pub downstream_tls: DownstreamTlsSettings,
    pub upstream_tls: UpstreamTlsSettings,
    pub request_codec_policy: Option<CodecPolicyId>,
    pub response_codec_policy: Option<CodecPolicyId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProxyListener {
    Forward(ForwardProxyListener),
    Reverse(ReverseProxyListener),
}

impl ProxyListener {
    #[must_use]
    pub const fn id(&self) -> ListenerId {
        match self {
            Self::Forward(listener) => listener.id,
            Self::Reverse(listener) => listener.id,
        }
    }

    #[must_use]
    pub const fn enabled(&self) -> bool {
        match self {
            Self::Forward(listener) => listener.enabled,
            Self::Reverse(listener) => listener.enabled,
        }
    }

    #[must_use]
    pub fn bind_endpoint(&self) -> (&str, u16) {
        match self {
            Self::Forward(listener) => (&listener.bind_address, listener.port),
            Self::Reverse(listener) => (&listener.bind_address, listener.port),
        }
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
    pub body_codec_policies: Vec<BodyCodecPolicy>,
    pub metadata_extractors: Vec<MetadataExtractor>,
    pub response_assertions: Vec<ResponseAssertion>,
    /// 规则通过 rule_* 用例维护，避免前端在 Workspace 表单中复制第二套规则编辑器。
    /// 字段仍属于领域聚合并参与导入导出，只不重复进入 Workspace 的 TypeScript DTO。
    #[specta(skip)]
    pub rules: Vec<Rule>,
    pub fault_presets: Vec<FaultPreset>,
    pub certificate_references: Vec<CertificateReference>,
}

impl Default for ProxyWorkspace {
    fn default() -> Self {
        Self {
            id: WorkspaceId::new(),
            name: "Untitled Workspace".into(),
            revision: Revision::INITIAL,
            listeners: vec![ProxyListener::Forward(ForwardProxyListener::default())],
            body_codec_policies: Vec::new(),
            metadata_extractors: Vec::new(),
            response_assertions: Vec::new(),
            rules: Vec::new(),
            fault_presets: Vec::new(),
            certificate_references: Vec::new(),
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
            self.listeners.iter().map(ProxyListener::id),
            "listeners",
            &mut error,
        );
        let mut enabled_endpoints = BTreeMap::new();
        for (index, listener) in self.listeners.iter().enumerate() {
            validate_listener(listener, index, &certificate_ids, &mut error);
            if listener.enabled() {
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
    let codec_ids = unique_ids(
        workspace.body_codec_policies.iter().map(|item| item.id),
        "body_codec_policies",
        error,
    );
    for (index, policy) in workspace.body_codec_policies.iter().enumerate() {
        validate_named_listener_refs(
            &policy.name,
            &policy.listener_ids,
            listener_ids,
            &format!("body_codec_policies.{index}"),
            error,
        );
    }
    for (index, listener) in workspace.listeners.iter().enumerate() {
        if let ProxyListener::Reverse(listener) = listener {
            for (field, policy) in [
                ("request_codec_policy", listener.request_codec_policy),
                ("response_codec_policy", listener.response_codec_policy),
            ] {
                if policy.is_some_and(|id| !codec_ids.contains(&id)) {
                    push_field_error(
                        error,
                        format!("listeners.{index}.{field}"),
                        "引用的 Body 编解码策略不存在",
                    );
                }
            }
        }
    }

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
}

fn validate_listener(
    listener: &ProxyListener,
    index: usize,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    error: &mut DomainError,
) {
    let prefix = format!("listeners.{index}");
    let (name, bind_address, port) = match listener {
        ProxyListener::Forward(value) => (&value.name, &value.bind_address, value.port),
        ProxyListener::Reverse(value) => (&value.name, &value.bind_address, value.port),
    };
    if name.trim().is_empty() {
        push_field_error(error, format!("{prefix}.name"), "监听器名称不能为空");
    }
    let bind_ip = bind_address.parse::<IpAddr>();
    if bind_ip.is_err() {
        push_field_error(
            error,
            format!("{prefix}.bind_address"),
            "绑定地址必须是有效 IP",
        );
    }
    if port == 0 {
        push_field_error(error, format!("{prefix}.port"), "监听端口必须大于 0");
    }

    match listener {
        ProxyListener::Forward(value) => {
            validate_forward_listener(value, bind_ip.ok(), certificate_ids, &prefix, error);
        }
        ProxyListener::Reverse(value) => {
            validate_reverse_listener(value, certificate_ids, &prefix, error);
        }
    }
}

fn validate_forward_listener(
    value: &ForwardProxyListener,
    bind_ip: Option<IpAddr>,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
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
    if bind_ip.is_some_and(|ip| !ip.is_loopback()) {
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

fn validate_reverse_listener(
    value: &ReverseProxyListener,
    certificate_ids: &BTreeSet<CertificateReferenceId>,
    prefix: &str,
    error: &mut DomainError,
) {
    if !is_valid_upstream_origin(&value.upstream_url) {
        push_field_error(
            error,
            format!("{prefix}.upstream_url"),
            "固定上游必须是 HTTP/HTTPS origin，不能包含路径、查询、片段或用户信息",
        );
    }
    if value.downstream_tls.enabled
        && value
            .downstream_tls
            .server_identity
            .is_none_or(|id| !certificate_ids.contains(&id))
    {
        push_field_error(
            error,
            format!("{prefix}.downstream_tls.server_identity"),
            "启用下游 TLS 时必须引用服务端身份",
        );
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
    }
    for (field, reference) in [
        ("server_trust", value.upstream_tls.server_trust),
        ("client_identity", value.upstream_tls.client_identity),
    ] {
        if reference.is_some_and(|id| !certificate_ids.contains(&id)) {
            push_field_error(
                error,
                format!("{prefix}.upstream_tls.{field}"),
                "上游 TLS 证书引用不存在",
            );
        }
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

    #[test]
    fn default_workspace_is_empty_safe_and_serializable() {
        let workspace = ProxyWorkspace::default();
        assert_eq!(workspace.listeners.len(), 1);
        let ProxyListener::Forward(listener) = &workspace.listeners[0] else {
            panic!("forward draft expected")
        };
        assert!(!listener.enabled);
        assert_eq!(listener.bind_address, "127.0.0.1");
        assert_eq!(listener.port, 8080);
        assert!(workspace.rules.is_empty());
        assert!(workspace.fault_presets.is_empty());
        workspace.validate().expect("safe draft must validate");
        let json = serde_json::to_string(&workspace).unwrap();
        for forbidden in ["private_key", "password", "pkcs12"] {
            assert!(!json.to_ascii_lowercase().contains(forbidden));
        }
    }

    #[test]
    fn non_loopback_forward_listener_requires_authentication_and_cidr() {
        let mut workspace = ProxyWorkspace::default();
        let ProxyListener::Forward(listener) = &mut workspace.listeners[0] else {
            unreachable!()
        };
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
    fn reverse_listener_accepts_generic_http_and_https_origins() {
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
    fn workspace_accepts_multiple_reverse_listener_mappings() {
        let reverse = |name: &str, port: u16, upstream_url: &str| {
            ProxyListener::Reverse(ReverseProxyListener {
                id: ListenerId::new(),
                name: name.into(),
                enabled: true,
                bind_address: "127.0.0.1".into(),
                port,
                upstream_url: upstream_url.into(),
                downstream_tls: DownstreamTlsSettings::default(),
                upstream_tls: UpstreamTlsSettings::default(),
                request_codec_policy: None,
                response_codec_policy: None,
            })
        };
        let workspace = ProxyWorkspace {
            listeners: vec![
                reverse(
                    "Transaction",
                    16_627,
                    "https://transaction.example.test:16627",
                ),
                reverse("DLL", 16_127, "https://dll.example.test:16127"),
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
        let ProxyListener::Forward(listener) = &mut workspace.listeners[0] else {
            unreachable!()
        };
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
    fn rejects_dangling_listener_and_certificate_references() {
        let mut workspace = ProxyWorkspace::default();
        workspace.body_codec_policies.push(BodyCodecPolicy {
            id: CodecPolicyId::new(),
            name: "text".into(),
            listener_ids: vec![ListenerId::new()],
            direction: BodyDirection::Both,
            codec: BodyCodecKind::Utf8,
        });
        let error = workspace.validate().unwrap_err();
        assert!(
            error
                .field_errors
                .contains_key("body_codec_policies.0.listener_ids.0")
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
