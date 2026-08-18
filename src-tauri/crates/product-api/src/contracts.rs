use std::fmt;

/// 宿主定义的监听器与上游默认信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductChannel {
    pub id: &'static str,
    pub display_name: &'static str,
    pub enabled_by_default: bool,
    pub listen_port: u16,
    pub upstream_url: &'static str,
}

/// 持久化和秘密保护命名空间，避免不同应用互相读取数据库或密钥。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductStorageNamespace {
    pub database_file_name: &'static str,
    pub secret_service: &'static str,
    pub secret_account: &'static str,
    pub secret_envelope_magic: &'static [u8; 5],
    pub secret_aad: &'static [u8],
}

/// 通用适配器展示时使用的宿主术语。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductLabels {
    pub client_name: &'static str,
    pub upstream_name: &'static str,
    pub fault_rule_name_prefix: &'static str,
}

/// 宿主为通用故障能力选择的展示元数据。
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

/// 请求分类器提取、供列表和规则使用的少量元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClassifiedRequest {
    pub request_id: Option<String>,
    pub request_type: Option<String>,
}

/// 保留原始字节的只读 Header 视图。
#[derive(Debug, Clone, Copy)]
pub struct ProductHeader<'a> {
    pub name: &'a [u8],
    pub value: &'a [u8],
}

/// 请求分类器可读取的一条完整请求视图。
#[derive(Debug, Clone, Copy)]
pub struct ProductMessageContext<'a> {
    pub channel_id: &'a str,
    pub start_line: &'a [u8],
    pub headers: &'a [ProductHeader<'a>],
    pub body: &'a [u8],
}

/// 从请求中提取宿主需要的元数据。
pub trait RequestClassifier: fmt::Debug + Send + Sync {
    fn classify(&self, message: ProductMessageContext<'_>) -> ClassifiedRequest;
}
