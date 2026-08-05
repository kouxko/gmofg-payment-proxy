//! 通用代理 Workspace 领域模型。
//!
//! Workspace 是桌面 UI、未来 TUI/CLI 和无界面测试共同使用的配置边界。这里仅保存
//! 可序列化配置与安全引用，不保存证书私钥、PKCS#12 密码、代理认证明文或文件内容。
//! 因此 `.intercept-workspace` 可以安全地经过统一导入导出流程，但真正的秘密仍由
//! infrastructure 根据 [`SecretReference`] 从系统密钥库中解析。

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use specta::Type;

use crate::{
    AndroidNetworkProfile, CertificateReferenceId, DomainError, ErrorCode, FaultPresetId,
    ListenerId, MetadataExtractorId, ResponseAssertionId, Revision, Rule, RuleAction, WorkspaceId,
};

mod validation;

pub use validation::{is_valid_cidr, is_valid_upstream_origin};
use validation::{push_field_error, unique_ids, validate_listener, validate_workspace_references};

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
    /// 允许动态签发的精确 DNS 或 `*.example.test` 域名模式。IP 字面量
    /// 必须由固定服务端身份的 SAN 覆盖，不参与基于 SNI 的动态签发。Android 透明代理路由
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

#[cfg(test)]
mod tests;
