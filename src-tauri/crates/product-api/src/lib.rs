//! 可复用代理核心与具体产品之间的策略边界。
//!
//! 本 crate 刻意只放契约。产品名称、默认通道、内置信任锚，以及显式启用的测试签名材料
//! 都由具体产品拥有，不能泄漏进通用领域层或代理运行时。

use std::{collections::BTreeSet, error::Error, fmt, net::IpAddr, sync::Arc};

/// 产品定义的监听器/上游信息。
///
/// 通用核心只把通道 ID 当作不透明标识；具体产品提供通道目录、显示名、端口和上游默认值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductChannel {
    pub id: &'static str,
    pub display_name: &'static str,
    pub enabled_by_default: bool,
    pub listen_port: u16,
    pub upstream_url: &'static str,
}

/// 产品独占的持久化和秘密保护命名空间，避免多个产品互相读取数据库或密钥。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductStorageNamespace {
    pub database_file_name: &'static str,
    pub secret_service: &'static str,
    pub secret_account: &'static str,
    pub secret_envelope_magic: &'static [u8; 5],
    pub secret_aad: &'static [u8],
}

/// 产品旧版本写入设置时使用的字段别名。
///
/// 通用持久化代码知道如何迁移通道目录，但不应知道具体产品曾使用哪些旧字段名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacySettingsChannelMapping {
    pub channel_id: &'static str,
    pub enabled_field: &'static str,
    pub port_field: &'static str,
    pub upstream_url_field: &'static str,
}

/// 产品持久化数据的兼容迁移元数据。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProductPersistenceMigrations {
    pub settings_channels: &'static [LegacySettingsChannelMapping],
    pub terminal_body_fields: &'static [&'static str],
}

/// 通用适配器展示时使用的产品术语。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductLabels {
    pub client_name: &'static str,
    pub upstream_name: &'static str,
    pub fault_rule_name_prefix: &'static str,
}

/// 产品为通用故障能力选择的展示元数据。
///
/// `id` 指向通用规则引擎实现的能力；产品决定暴露哪些能力以及如何描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductFaultTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub stage_text: &'static str,
    pub behavior_text: &'static str,
    pub affected_party_text: &'static str,
    pub default_channel_id: &'static str,
    pub risk_text: &'static str,
}

pub const STANDARD_FAULT_CAPABILITY_IDS: &[&str] = &[
    "reject_tls_handshake",
    "disconnect_before_upstream",
    "request_delay",
    "modify_request_json",
    "drop_upstream_response",
    "upstream_connect_timeout",
    "upstream_write_timeout",
    "upstream_read_timeout",
    "response_delay",
    "custom_http_status",
    "mock_json",
    "invalid_json",
    "wrong_content_length",
    "truncate_response",
    "throttle_upstream",
    "throttle_downstream",
    "jitter_upstream",
    "jitter_downstream",
    "intermittent_upstream",
    "intermittent_downstream",
    "disconnect_upstream_mid_body",
    "disconnect_downstream_mid_body",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// 产品分类器从请求中提取、供列表和规则使用的少量元数据。
pub struct ClassifiedRequest {
    pub request_id: Option<String>,
    pub request_type: Option<String>,
}

#[derive(Debug, Clone, Copy)]
/// 保留原始字节的只读 Header 视图。
pub struct ProductHeader<'a> {
    pub name: &'a [u8],
    pub value: &'a [u8],
}

#[derive(Debug, Clone, Copy)]
/// 产品分类器可读取的一条完整请求视图，所有借用仅在本次调用期间有效。
pub struct ProductMessageContext<'a> {
    pub channel_id: &'a str,
    pub start_line: &'a [u8],
    pub headers: &'a [ProductHeader<'a>],
    pub body: &'a [u8],
}

/// 从请求中提取产品特有元数据。
///
/// 通用传输和存储代码永远不需要知道产品 JSON 字段名。
pub trait RequestClassifier: fmt::Debug + Send + Sync {
    fn classify(&self, message: ProductMessageContext<'_>) -> ClassifiedRequest;
}

/// 编解码器和产品扩展点返回的稳定产品边界错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductError {
    pub code: &'static str,
    pub message: String,
}

impl ProductError {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for ProductError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for ProductError {}

/// 产品选择的 Body 编解码契约，例如 Payment 使用 Shift-JIS。
pub trait BodyCodec: fmt::Debug + Send + Sync {
    /// 稳定编解码器 ID，便于日志和诊断识别。
    fn id(&self) -> &'static str;

    /// 给人阅读的编码名称。
    fn name(&self) -> &'static str;

    /// 将线上字节无损解码为可编辑文本；存在非法字节时必须失败。
    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError>;

    /// 将用户文本无损编码回产品字节；有不可表示字符时必须失败，不能替换为问号。
    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError>;
}

/// 通用证书适配器展示的产品自定义文案。
#[derive(Debug, Clone, Copy)]
pub struct CertificateLabels {
    pub root_name: &'static str,
    pub root_usage: &'static str,
    pub leaf_name: &'static str,
    pub leaf_usage: &'static str,
    pub client_identity_name: &'static str,
    pub client_identity_usage: &'static str,
    pub upstream_name: &'static str,
    pub upstream_bundled_usage: &'static str,
    pub upstream_override_usage: &'static str,
    pub ready_status: &'static str,
    pub incomplete_status: &'static str,
    pub already_exists_message: &'static str,
    pub export_cancelled_message: &'static str,
    pub export_success_message: &'static str,
}

/// 外层装配选择的证书展示文案与可选上游信任锚。
pub trait ProductCertificatePolicy: fmt::Debug + Send + Sync {
    /// 产品随包携带的默认上游信任锚，可由用户导入文件替换。
    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]>;

    /// 旧式、产品固定的运行模式可把共享上游客户端身份视为全局启动前置项。
    /// 通用 Intercept Proxy 按入口引用身份，因此默认不要求这个可选材料。
    fn requires_global_client_identity(&self) -> bool {
        false
    }

    /// 旧式、产品固定的运行模式可把上游 CA 视为全局启动前置项。
    /// 通用 Intercept Proxy 按入口选择系统信任或 CA 引用，因此默认不要求。
    fn requires_global_upstream_ca(&self) -> bool {
        false
    }

    fn labels(&self) -> CertificateLabels;
}

/// 注入到 UI 无关 Rust Host 中的产品配置总契约。
pub trait ProductProfile: fmt::Debug + Send + Sync {
    /// 稳定、机器可读的产品 ID。
    fn id(&self) -> &'static str;

    /// 界面显示的产品名称。
    fn name(&self) -> &'static str;

    /// 产品支持的全部监听通道及默认上游。
    fn channels(&self) -> &'static [ProductChannel];

    /// 数据库、Keychain/DPAPI 的隔离命名空间。
    fn storage(&self) -> ProductStorageNamespace;

    fn persistence_migrations(&self) -> ProductPersistenceMigrations {
        ProductPersistenceMigrations::default()
    }

    fn labels(&self) -> ProductLabels;

    /// 产品允许用户快速配置的故障能力。
    fn fault_templates(&self) -> &'static [ProductFaultTemplate];

    fn request_classifier(&self) -> Arc<dyn RequestClassifier>;

    fn certificates(&self) -> &dyn ProductCertificatePolicy;

    fn body_codec(&self) -> Arc<dyn BodyCodec>;
}

/// 在 Host 打开存储或启动后台任务前验证静态产品契约。
pub fn validate_product_profile(product: &dyn ProductProfile) -> Result<(), ProductError> {
    let channels = product.channels();
    let mut ids = BTreeSet::new();
    let mut enabled_ports = BTreeSet::new();
    for channel in channels {
        validate_channel_id(channel.id)?;
        if !ids.insert(channel.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("duplicate product channel ID {:?}", channel.id),
            ));
        }
        if channel.display_name.trim().is_empty() {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("channel {:?} has an empty display name", channel.id),
            ));
        }
        if channel.enabled_by_default
            && (channel.listen_port == 0 || !enabled_ports.insert(channel.listen_port))
        {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "enabled channel {:?} has a zero or duplicate listen port",
                    channel.id
                ),
            ));
        }
        if !valid_https_origin(channel.upstream_url) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "channel {:?} upstream must be an HTTPS origin without path, query, fragment, or userinfo",
                    channel.id
                ),
            ));
        }
    }

    validate_product_storage(product.storage())?;
    validate_persistence_migrations(product.persistence_migrations(), &ids)?;
    let mut template_ids = BTreeSet::new();
    for template in product.fault_templates() {
        if template.id.is_empty() || !template_ids.insert(template.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!("fault template ID {:?} is empty or duplicated", template.id),
            ));
        }
        if !STANDARD_FAULT_CAPABILITY_IDS.contains(&template.id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "fault template {:?} names an unknown capability",
                    template.id
                ),
            ));
        }
        if !ids.contains(template.default_channel_id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "fault template {:?} references unknown channel {:?}",
                    template.id, template.default_channel_id
                ),
            ));
        }
    }
    validate_product_labels(product.labels())
}

fn validate_persistence_migrations(
    migrations: ProductPersistenceMigrations,
    channel_ids: &BTreeSet<&str>,
) -> Result<(), ProductError> {
    let mut settings_fields = BTreeSet::new();
    for mapping in migrations.settings_channels {
        if !channel_ids.contains(mapping.channel_id) {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                format!(
                    "legacy settings mapping references unknown channel {:?}",
                    mapping.channel_id
                ),
            ));
        }
        for field in [
            mapping.enabled_field,
            mapping.port_field,
            mapping.upstream_url_field,
        ] {
            if field.trim().is_empty() || !settings_fields.insert(field) {
                return Err(ProductError::new(
                    "PRODUCT_PROFILE_INVALID",
                    "legacy settings field names must be non-empty and unique",
                ));
            }
        }
    }
    let mut terminal_fields = BTreeSet::new();
    for field in migrations.terminal_body_fields {
        if field.trim().is_empty() || !terminal_fields.insert(*field) || *field == "body_bytes" {
            return Err(ProductError::new(
                "PRODUCT_PROFILE_INVALID",
                "legacy terminal body fields must be non-empty, unique aliases",
            ));
        }
    }
    Ok(())
}

fn validate_product_storage(storage: ProductStorageNamespace) -> Result<(), ProductError> {
    if [
        storage.database_file_name,
        storage.secret_service,
        storage.secret_account,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || storage.secret_aad.is_empty()
    {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product storage namespace must be non-empty",
        ));
    }
    if !valid_database_file_name(storage.database_file_name) {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product database file name must be one portable file-name component",
        ));
    }
    Ok(())
}

fn validate_product_labels(labels: ProductLabels) -> Result<(), ProductError> {
    if [
        labels.client_name,
        labels.upstream_name,
        labels.fault_rule_name_prefix,
    ]
    .iter()
    .any(|value| value.trim().is_empty())
    {
        return Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            "product labels must be non-empty",
        ));
    }
    Ok(())
}

fn validate_channel_id(value: &str) -> Result<(), ProductError> {
    let valid = !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    if valid {
        Ok(())
    } else {
        Err(ProductError::new(
            "PRODUCT_PROFILE_INVALID",
            format!("invalid product channel ID {value:?}"),
        ))
    }
}

fn valid_database_file_name(value: &str) -> bool {
    !matches!(value, "." | "..")
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'/' | b'\\' | b':' | 0))
}

/// 校验产品配置声明的静态上游 origin。
///
/// runtime 会保留下游 request-target，产品配置中的 path/query 无法安全合并且会被丢弃。
/// 此契约必须与领域设置校验和 runtime 端点解析器保持一致；单个结尾 `/` 代表空路径。
fn valid_https_origin(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("https://") else {
        return false;
    };
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') {
        return false;
    }
    if !matches!(&rest[authority_end..], "" | "/") {
        return false;
    }

    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        let Some(end) = bracketed.find(']') else {
            return false;
        };
        let host = &bracketed[..end];
        let suffix = &bracketed[end + 1..];
        if !valid_optional_port(suffix) {
            return false;
        }
        return host
            .parse::<IpAddr>()
            .is_ok_and(|address| matches!(address, IpAddr::V6(_)));
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || !valid_port(port) {
            return false;
        }
        host
    } else {
        authority
    };
    host.parse::<IpAddr>().is_ok()
        || (!host.is_empty()
            && host.len() <= 253
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            }))
}

fn valid_optional_port(value: &str) -> bool {
    value.is_empty() || value.strip_prefix(':').is_some_and(valid_port)
}

fn valid_port(value: &str) -> bool {
    value.parse::<u16>().is_ok_and(|port| port > 0)
}

/// Intercept Proxy 的无业务默认配置。
///
/// 这个配置只负责为仍在迁移期内的 Host 提供稳定的存储、安全命名空间和通用文案。
/// 真正可编辑的监听器、编码器、提取器与断言由 `domain::ProxyWorkspace` 持有，不再由
/// 编译期产品适配器决定。产品适配器不声明运行通道；首次启动的正向代理草稿由
/// Workspace 创建，并且只在“入口配置”中编辑和启动。
#[derive(Debug, Default, Clone, Copy)]
pub struct InterceptProxyProfile;

#[derive(Debug, Default)]
struct StrictUtf8BodyCodec;

#[derive(Debug, Default)]
struct EmptyRequestClassifier;

impl BodyCodec for StrictUtf8BodyCodec {
    fn id(&self) -> &'static str {
        "utf-8"
    }

    fn name(&self) -> &'static str {
        "UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        String::from_utf8(bytes.to_vec())
            .map_err(|error| ProductError::new("BODY_DECODE_FAILED", error.to_string()))
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

impl RequestClassifier for EmptyRequestClassifier {
    fn classify(&self, _: ProductMessageContext<'_>) -> ClassifiedRequest {
        ClassifiedRequest::default()
    }
}

impl ProductCertificatePolicy for InterceptProxyProfile {
    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]> {
        None
    }

    fn labels(&self) -> CertificateLabels {
        CertificateLabels {
            root_name: "Intercept Proxy Root CA",
            root_usage: "仅用于用户显式允许的 HTTPS MITM 目标",
            leaf_name: "动态代理服务端证书",
            leaf_usage: "按监听地址或 CONNECT authority 动态签发",
            client_identity_name: "上游客户端身份",
            client_identity_usage: "可选的反向代理 mTLS PKCS12 身份",
            upstream_name: "上游 CA",
            upstream_bundled_usage: "未配置；使用系统信任或监听器显式 CA",
            upstream_override_usage: "用户为反向监听器导入的上游 CA",
            ready_status: "证书已就绪",
            incomplete_status: "证书尚未初始化",
            already_exists_message: "当前安装实例已经存在 Root CA。",
            export_cancelled_message: "已取消导出 Root CA。",
            export_success_message: "Root CA 已导出。",
        }
    }
}

impl ProductProfile for InterceptProxyProfile {
    fn id(&self) -> &'static str {
        "intercept-proxy"
    }

    fn name(&self) -> &'static str {
        "Intercept Proxy"
    }

    fn channels(&self) -> &'static [ProductChannel] {
        &[]
    }

    fn storage(&self) -> ProductStorageNamespace {
        ProductStorageNamespace {
            database_file_name: "intercept-proxy.sqlite3",
            secret_service: "com.interceptproxy.desktop",
            secret_account: "intercept-proxy-secrets",
            secret_envelope_magic: b"IPX02",
            secret_aad: b"com.interceptproxy.desktop/v2",
        }
    }

    fn labels(&self) -> ProductLabels {
        ProductLabels {
            client_name: "客户端",
            upstream_name: "上游服务",
            fault_rule_name_prefix: "故障规则 · ",
        }
    }

    fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
        &[]
    }

    fn request_classifier(&self) -> Arc<dyn RequestClassifier> {
        Arc::new(EmptyRequestClassifier)
    }

    fn certificates(&self) -> &dyn ProductCertificatePolicy {
        self
    }

    fn body_codec(&self) -> Arc<dyn BodyCodec> {
        Arc::new(StrictUtf8BodyCodec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestProfile {
        channels: &'static [ProductChannel],
        storage: ProductStorageNamespace,
        faults: &'static [ProductFaultTemplate],
    }

    #[derive(Debug)]
    struct Utf8Codec;

    #[derive(Debug)]
    struct EmptyClassifier;

    const VALID_CHANNELS: &[ProductChannel] = &[
        channel("alpha_2.v1", 20_001, "https://alpha.example.test"),
        channel("A-channel", 20_002, "https://beta.example.test"),
    ];
    const DUPLICATE_PORTS: &[ProductChannel] = &[
        channel("alpha", 20_001, "https://alpha.example.test"),
        channel("beta", 20_001, "https://beta.example.test"),
    ];
    const INVALID_ID: &[ProductChannel] =
        &[channel("-alpha", 20_001, "https://alpha.example.test")];
    const INVALID_URL: &[ProductChannel] = &[channel("alpha", 20_001, "https://:bad")];
    const VALID_FAULTS: &[ProductFaultTemplate] = &[fault("request_delay", "alpha_2.v1")];
    const UNKNOWN_CHANNEL_FAULTS: &[ProductFaultTemplate] = &[fault("request_delay", "missing")];
    const DUPLICATE_FAULTS: &[ProductFaultTemplate] = &[
        fault("request_delay", "alpha_2.v1"),
        fault("request_delay", "A-channel"),
    ];
    const UNKNOWN_CAPABILITY: &[ProductFaultTemplate] = &[fault("not-supported", "alpha_2.v1")];

    const fn channel(id: &'static str, port: u16, url: &'static str) -> ProductChannel {
        ProductChannel {
            id,
            display_name: id,
            enabled_by_default: true,
            listen_port: port,
            upstream_url: url,
        }
    }

    const fn fault(id: &'static str, channel: &'static str) -> ProductFaultTemplate {
        ProductFaultTemplate {
            id,
            name: id,
            stage_text: "request",
            behavior_text: "delay",
            affected_party_text: "client",
            default_channel_id: channel,
            risk_text: "low",
        }
    }

    const fn storage() -> ProductStorageNamespace {
        ProductStorageNamespace {
            database_file_name: "test.sqlite3",
            secret_service: "com.example.test",
            secret_account: "key",
            secret_envelope_magic: b"TSTK1",
            secret_aad: b"test/aad",
        }
    }

    impl BodyCodec for Utf8Codec {
        fn id(&self) -> &'static str {
            "utf-8"
        }

        fn name(&self) -> &'static str {
            "UTF-8"
        }

        fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
            String::from_utf8(bytes.to_vec())
                .map_err(|error| ProductError::new("DECODE", error.to_string()))
        }

        fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
            Ok(text.as_bytes().to_vec())
        }
    }

    impl RequestClassifier for EmptyClassifier {
        fn classify(&self, _: ProductMessageContext<'_>) -> ClassifiedRequest {
            ClassifiedRequest::default()
        }
    }

    impl ProductCertificatePolicy for TestProfile {
        fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]> {
            None
        }

        fn labels(&self) -> CertificateLabels {
            CertificateLabels {
                root_name: "root",
                root_usage: "test",
                leaf_name: "leaf",
                leaf_usage: "test",
                client_identity_name: "identity",
                client_identity_usage: "test",
                upstream_name: "upstream",
                upstream_bundled_usage: "test",
                upstream_override_usage: "test",
                ready_status: "ready",
                incomplete_status: "incomplete",
                already_exists_message: "exists",
                export_cancelled_message: "cancelled",
                export_success_message: "exported",
            }
        }
    }

    impl ProductProfile for TestProfile {
        fn id(&self) -> &'static str {
            "test"
        }

        fn name(&self) -> &'static str {
            "Test"
        }

        fn channels(&self) -> &'static [ProductChannel] {
            self.channels
        }

        fn storage(&self) -> ProductStorageNamespace {
            self.storage
        }

        fn labels(&self) -> ProductLabels {
            ProductLabels {
                client_name: "Client",
                upstream_name: "Upstream",
                fault_rule_name_prefix: "Fault · ",
            }
        }

        fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
            self.faults
        }

        fn request_classifier(&self) -> Arc<dyn RequestClassifier> {
            Arc::new(EmptyClassifier)
        }

        fn certificates(&self) -> &dyn ProductCertificatePolicy {
            self
        }

        fn body_codec(&self) -> Arc<dyn BodyCodec> {
            Arc::new(Utf8Codec)
        }
    }

    #[test]
    fn profile_validation_accepts_runtime_channel_id_grammar() {
        validate_product_profile(&TestProfile {
            channels: VALID_CHANNELS,
            storage: storage(),
            faults: VALID_FAULTS,
        })
        .unwrap();
    }

    #[test]
    fn profile_validation_rejects_every_cross_boundary_invariant() {
        let mut empty_storage = storage();
        empty_storage.secret_service = "";
        for profile in [
            TestProfile {
                channels: INVALID_ID,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: DUPLICATE_PORTS,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: INVALID_URL,
                storage: storage(),
                faults: &[],
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: empty_storage,
                faults: VALID_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: UNKNOWN_CHANNEL_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: DUPLICATE_FAULTS,
            },
            TestProfile {
                channels: VALID_CHANNELS,
                storage: storage(),
                faults: UNKNOWN_CAPABILITY,
            },
        ] {
            assert_eq!(
                validate_product_profile(&profile).unwrap_err().code,
                "PRODUCT_PROFILE_INVALID"
            );
        }
    }

    #[test]
    fn profile_validation_accepts_an_empty_compile_time_channel_catalog() {
        validate_product_profile(&TestProfile {
            channels: &[],
            storage: storage(),
            faults: &[],
        })
        .expect("dynamic Workspace listeners do not require product channels");
    }

    #[test]
    fn profile_validation_rejects_database_paths_outside_the_product_directory() {
        for database_file_name in [
            "../escape.sqlite3",
            "/tmp/escape.sqlite3",
            r"..\escape.sqlite3",
            r"C:\escape.sqlite3",
            ".",
            "..",
        ] {
            let mut invalid_storage = storage();
            invalid_storage.database_file_name = database_file_name;
            let error = validate_product_profile(&TestProfile {
                channels: VALID_CHANNELS,
                storage: invalid_storage,
                faults: VALID_FAULTS,
            })
            .expect_err("database path must remain inside the product data directory");
            assert_eq!(error.code, "PRODUCT_PROFILE_INVALID");
        }
    }

    #[test]
    fn product_channels_accept_only_https_origins() {
        for invalid in [
            "http://alpha.example.test",
            "https://alpha.example.test/base",
            "https://alpha.example.test?mode=test",
            "https://alpha.example.test/#fragment",
            "https://user@alpha.example.test",
            " https://alpha.example.test ",
        ] {
            assert!(
                !valid_https_origin(invalid),
                "{invalid:?} must not pass the product profile boundary"
            );
        }
        for valid in [
            "https://alpha.example.test",
            "https://alpha.example.test/",
            "https://alpha.example.test:443",
            "https://[2001:db8::1]:443",
        ] {
            assert!(
                valid_https_origin(valid),
                "{valid:?} is a valid HTTPS origin"
            );
        }
    }

    #[test]
    fn intercept_profile_is_clean_and_declares_no_product_channels() {
        let profile = InterceptProxyProfile;
        validate_product_profile(&profile).expect("generic profile must be valid");
        assert_eq!(profile.name(), "Intercept Proxy");
        assert_eq!(
            profile.storage().database_file_name,
            "intercept-proxy.sqlite3"
        );
        assert!(profile.channels().is_empty());
        assert!(profile.certificates().bundled_upstream_ca_pem().is_none());
    }
}
