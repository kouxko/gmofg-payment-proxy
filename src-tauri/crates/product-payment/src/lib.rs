//! GMO-FG Payment 产品配置。
//!
//! 产品证书资源放在这里，使通用代理基础设施无需知道 Payment 名称或测试私密材料。默认
//! 依赖图只包含产品契约与编解码器。独立真机代理会装配 runtime/infrastructure，因此刻意
//! 隔离在 `real-device-tool` Cargo feature 后面，不污染可复用产品策略库。

use encoding_rs::SHIFT_JIS;
use gmofg_proxy_product_api::{
    BodyCodec, CertificateLabels, ClassifiedRequest, EmbeddedTestCertificateAuthority,
    LegacySettingsChannelMapping, ProductCertificatePolicy, ProductChannel, ProductError,
    ProductFaultTemplate, ProductLabels, ProductMessageContext, ProductPersistenceMigrations,
    ProductProfile, ProductStorageNamespace, RequestClassifier,
};

const TEST_ROOT_CA_CERTIFICATE_PEM: &[u8] =
    include_bytes!("../../../../test-support/certificates/unified-test-proxy-root-ca.crt");
const TEST_ROOT_CA_SIGNING_KEY_PEM: &str = include_str!(
    "../../../../test-support/certificates/unified-test-proxy-root-ca-signing-key.TEST-ONLY.txt"
);
const BUNDLED_PAYMENT_SERVER_CERTIFICATES_PEM: &[u8] =
    include_bytes!("../../../../test-support/certificates/bundled-payment-server.crt");

/// 控制当前进程是否允许使用内置的隔离测试签名私钥。
///
/// 默认必须关闭；只有明确创建真机隔离测试配置时才能启用，不能让生产路径意外签发证书。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedTestSigning {
    #[default]
    Disabled,
    EnabledForIsolatedTesting,
}

#[derive(Debug, Default)]
/// GMO-FG Payment 对通用代理核心提供的全部产品策略。
pub struct PaymentProductProfile {
    embedded_test_signing: EmbeddedTestSigning,
}

#[derive(Debug, Default)]
/// Payment Body 的 Shift-JIS 无损编解码器。
struct PaymentBodyCodec;

#[derive(Debug, Default)]
/// 从 Payment JSON/Header 中提取请求 ID 和交易类型的分类器。
struct PaymentRequestClassifier;

const PAYMENT_CHANNELS: &[ProductChannel] = &[
    ProductChannel {
        id: "transaction",
        display_name: "交易",
        enabled_by_default: true,
        listen_port: 16_627,
        upstream_url: "https://https.gmo-fg.net:16627",
    },
    ProductChannel {
        id: "dll",
        display_name: "DLL",
        enabled_by_default: true,
        listen_port: 16_127,
        upstream_url: "https://https.gmo-fg.net:16127",
    },
];

const PAYMENT_LEGACY_SETTINGS_CHANNELS: &[LegacySettingsChannelMapping] = &[
    LegacySettingsChannelMapping {
        channel_id: "transaction",
        enabled_field: "transaction_enabled",
        port_field: "transaction_port",
        upstream_url_field: "upstream_transaction_url",
    },
    LegacySettingsChannelMapping {
        channel_id: "dll",
        enabled_field: "dll_enabled",
        port_field: "dll_port",
        upstream_url_field: "upstream_dll_url",
    },
];

const PAYMENT_LEGACY_TERMINAL_BODY_FIELDS: &[&str] = &["shift_jis_body"];

macro_rules! payment_fault {
    ($id:literal, $name:literal, $stage:literal, $behavior:literal, $affected:literal, $risk:literal) => {
        ProductFaultTemplate {
            id: $id,
            name: $name,
            stage_text: $stage,
            behavior_text: $behavior,
            affected_party_text: $affected,
            default_channel_id: "transaction",
            risk_text: $risk,
        }
    };
}

const PAYMENT_FAULT_TEMPLATES: &[ProductFaultTemplate] = &[
    payment_fault!(
        "reject_tls_handshake",
        "拒绝 TLS 握手",
        "TLS 握手阶段",
        "在 HTTP 消息进入规则管线前拒绝客户端握手",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "disconnect_before_upstream",
        "不连接上游并断开",
        "请求阶段",
        "不建立上游连接并关闭 App 连接",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "request_delay",
        "请求前延迟/超时",
        "请求阶段",
        "转发前等待指定时间",
        "Payment App 与 GMO-FG Server",
        "中"
    ),
    payment_fault!(
        "modify_request_json",
        "修改请求 JSON",
        "请求阶段",
        "修改指定 JSON 字段",
        "GMO-FG Server",
        "中"
    ),
    payment_fault!(
        "drop_upstream_response",
        "发送上游后丢弃响应",
        "请求阶段",
        "读取响应后不返回 App 并断开",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "upstream_connect_timeout",
        "上游连接超时",
        "请求阶段",
        "保持上游连接直至超时",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "upstream_write_timeout",
        "上游写入超时",
        "请求阶段",
        "连接上游后在写入请求时保持至超时",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "upstream_read_timeout",
        "上游读取超时",
        "请求阶段",
        "写入请求后在读取上游响应时保持至超时",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "response_delay",
        "响应延迟",
        "响应阶段",
        "返回 App 前等待指定时间",
        "Payment App",
        "中"
    ),
    payment_fault!(
        "custom_http_status",
        "自定义 HTTP 状态",
        "响应阶段",
        "返回指定 HTTP 状态码",
        "Payment App",
        "中"
    ),
    payment_fault!(
        "mock_json",
        "Mock Shift-JIS JSON",
        "请求阶段",
        "绕过上游并返回 Mock",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "invalid_json",
        "非法 JSON",
        "响应阶段",
        "返回可编码但语法非法的 JSON",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "wrong_content_length",
        "错误 Content-Length",
        "响应阶段",
        "声明长度与真实 Body 不一致",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "truncate_response",
        "截断响应",
        "响应阶段",
        "发送前 N 字节后断开",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "throttle_upstream",
        "上行限速",
        "请求阶段",
        "按指定速率分块发送请求 Body",
        "GMO-FG Server",
        "中"
    ),
    payment_fault!(
        "throttle_downstream",
        "下行限速",
        "响应阶段",
        "按指定速率分块返回响应 Body",
        "Payment App",
        "中"
    ),
    payment_fault!(
        "jitter_upstream",
        "上行抖动",
        "请求阶段",
        "请求 Body 每个分块发送前加入确定性随机抖动",
        "GMO-FG Server",
        "中"
    ),
    payment_fault!(
        "jitter_downstream",
        "下行抖动",
        "响应阶段",
        "响应 Body 每个分块发送前加入确定性随机抖动",
        "Payment App",
        "中"
    ),
    payment_fault!(
        "intermittent_upstream",
        "上行间歇通断",
        "请求阶段",
        "按可用窗口和阻断窗口循环发送请求 Body",
        "GMO-FG Server",
        "高"
    ),
    payment_fault!(
        "intermittent_downstream",
        "下行间歇通断",
        "响应阶段",
        "按可用窗口和阻断窗口循环返回响应 Body",
        "Payment App",
        "高"
    ),
    payment_fault!(
        "disconnect_upstream_mid_body",
        "上行 Body 中途断连",
        "请求阶段",
        "发送指定字节数后中止上游请求",
        "GMO-FG Server",
        "高"
    ),
    payment_fault!(
        "disconnect_downstream_mid_body",
        "下行 Body 中途断连",
        "响应阶段",
        "返回指定字节数后中止 App 响应",
        "Payment App",
        "高"
    ),
];

impl PaymentProductProfile {
    #[must_use]
    pub const fn new(embedded_test_signing: EmbeddedTestSigning) -> Self {
        Self {
            embedded_test_signing,
        }
    }

    /// 创建专用于 Payment 代理隔离测试工具的配置。
    ///
    /// 这是唯一开启内置测试 CA 私钥的公开构造路径，正常 `default()` 仍保持关闭。
    #[must_use]
    pub const fn isolated_test_tool() -> Self {
        Self::new(EmbeddedTestSigning::EnabledForIsolatedTesting)
    }
}

impl ProductProfile for PaymentProductProfile {
    fn id(&self) -> &'static str {
        "gmofg-payment"
    }

    fn name(&self) -> &'static str {
        "GMO-FG Payment"
    }

    fn channels(&self) -> &'static [ProductChannel] {
        PAYMENT_CHANNELS
    }

    fn storage(&self) -> ProductStorageNamespace {
        ProductStorageNamespace {
            database_file_name: "gmofg-payment-proxy.sqlite3",
            secret_service: "com.gmofg.payment-proxy",
            secret_account: "secret-protection-master-key-v1",
            secret_envelope_magic: b"GMPK1",
            secret_aad: b"gmofg-payment-proxy/keychain-envelope/v1",
        }
    }

    fn persistence_migrations(&self) -> ProductPersistenceMigrations {
        ProductPersistenceMigrations {
            settings_channels: PAYMENT_LEGACY_SETTINGS_CHANNELS,
            terminal_body_fields: PAYMENT_LEGACY_TERMINAL_BODY_FIELDS,
        }
    }

    fn labels(&self) -> ProductLabels {
        ProductLabels {
            client_name: "Payment App",
            upstream_name: "GMO-FG Server",
            fault_rule_name_prefix: "故障模拟·",
        }
    }

    fn fault_templates(&self) -> &'static [ProductFaultTemplate] {
        PAYMENT_FAULT_TEMPLATES
    }

    fn request_classifier(&self) -> std::sync::Arc<dyn RequestClassifier> {
        std::sync::Arc::new(PaymentRequestClassifier)
    }

    fn certificates(&self) -> &dyn ProductCertificatePolicy {
        self
    }

    fn body_codec(&self) -> std::sync::Arc<dyn BodyCodec> {
        std::sync::Arc::new(PaymentBodyCodec)
    }
}

impl RequestClassifier for PaymentRequestClassifier {
    fn classify(&self, message: ProductMessageContext<'_>) -> ClassifiedRequest {
        // 请求 ID 优先取 HTTP Header。这样即使 Body 不是合法 Shift-JIS/JSON，抓包仍能
        // 利用网关提供的关联 ID；只有 Header 缺失时才回退到产品 JSON 字段。
        let header_request_id = message
            .headers
            .iter()
            .find(|header| {
                [
                    b"x-request-id".as_slice(),
                    b"request-id",
                    b"x-correlation-id",
                ]
                .iter()
                .any(|name| header.name.eq_ignore_ascii_case(name))
            })
            .and_then(|header| std::str::from_utf8(header.value).ok())
            .map(str::to_owned);
        let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(message.body);
        if had_errors {
            // 分类只是辅助元数据，不能因无法解码而拒绝真实网络报文；返回已有 Header
            // 信息并让通用代理继续按原始字节转发。
            return ClassifiedRequest {
                request_id: header_request_id,
                request_type: None,
            };
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&decoded) else {
            return ClassifiedRequest {
                request_id: header_request_id,
                request_type: None,
            };
        };
        ClassifiedRequest {
            request_id: header_request_id.or_else(|| {
                ["RequestID", "requestId", "request_id", "reqId"]
                    .into_iter()
                    .find_map(|key| value.get(key))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            }),
            request_type: ["TransactionType", "transactionType", "requestType"]
                .into_iter()
                .find_map(|key| value.get(key))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
        }
    }
}

impl BodyCodec for PaymentBodyCodec {
    fn id(&self) -> &'static str {
        "shift-jis"
    }

    fn name(&self) -> &'static str {
        "Shift-JIS"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError> {
        // `decode_without_bom_handling` 符合 Payment 协议：Body 不使用 Unicode BOM。
        // `had_errors` 必须作为失败返回，不能让替换字符进入用户编辑并破坏原始报文。
        let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);
        if had_errors {
            return Err(ProductError::new(
                "SHIFT_JIS_DECODE_FAILED",
                "body contains an invalid Shift-JIS byte sequence",
            ));
        }
        Ok(decoded.into_owned())
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError> {
        // encoding_rs 可能用替换字符处理不可表示文本，因此必须检查 `had_errors`，
        // 例如 Emoji 不能悄悄变为 `?` 后发送到 GMO-FG Server。
        let (encoded, _encoding, had_errors) = SHIFT_JIS.encode(text);
        if had_errors {
            return Err(ProductError::new(
                "SHIFT_JIS_ENCODE_FAILED",
                "text cannot be represented losslessly in Shift-JIS",
            ));
        }
        Ok(encoded.into_owned())
    }
}

impl ProductCertificatePolicy for PaymentProductProfile {
    fn public_root_ca_pem(&self) -> &'static [u8] {
        TEST_ROOT_CA_CERTIFICATE_PEM
    }

    fn embedded_test_authority(&self) -> Option<EmbeddedTestCertificateAuthority> {
        (self.embedded_test_signing == EmbeddedTestSigning::EnabledForIsolatedTesting).then_some(
            EmbeddedTestCertificateAuthority {
                public_certificate_pem: TEST_ROOT_CA_CERTIFICATE_PEM,
                signing_key_pem: TEST_ROOT_CA_SIGNING_KEY_PEM,
                required_subject_marker: "TEST ONLY",
            },
        )
    }

    fn bundled_upstream_ca_pem(&self) -> Option<&'static [u8]> {
        Some(BUNDLED_PAYMENT_SERVER_CERTIFICATES_PEM)
    }

    fn labels(&self) -> CertificateLabels {
        CertificateLabels {
            root_name: "统一测试 Root CA",
            root_usage: "仅用于隔离测试环境签发 Proxy 服务端证书",
            leaf_name: "Proxy 叶子证书",
            leaf_usage: "App → Proxy TLS 服务端身份",
            client_identity_name: "共享 PKCS12",
            client_identity_usage: "Proxy → Server 客户端身份及 App 指纹",
            upstream_name: "上游 CA",
            upstream_bundled_usage: "验证 GMO-FG Server（内置 Payment server.crt）",
            upstream_override_usage: "验证 GMO-FG Server（用户替换）",
            ready_status: "证书已就绪",
            incomplete_status: "证书配置不完整",
            already_exists_message: "统一测试 Root CA 或 Proxy 叶子证书已存在，请使用“重签服务端证书”。",
            export_cancelled_message: "已取消统一测试 Root CA 导出。",
            export_success_message: "统一测试 Root CA 公开证书已导出，未包含私钥；可用于测试版 Payment 构建。",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_signing_key_is_fail_closed() {
        let profile = PaymentProductProfile::default();
        assert!(profile.embedded_test_authority().is_none());
        assert!(
            profile
                .public_root_ca_pem()
                .starts_with(b"-----BEGIN CERTIFICATE-----")
        );
    }

    #[test]
    fn generic_certificate_infrastructure_does_not_embed_a_signing_key() {
        let generic_certificate_source = include_str!("../../infrastructure/src/certificates.rs");
        let generic_adapter_source =
            include_str!("../../infrastructure/src/adapters/certificates.rs");
        let generic_manifest = include_str!("../../infrastructure/Cargo.toml");
        let normal_dependencies = generic_manifest
            .split("[dev-dependencies]")
            .next()
            .expect("normal dependency section");

        for source in [generic_certificate_source, generic_adapter_source] {
            assert!(!source.contains("include_str!"));
            assert!(!source.contains("signing-key.TEST-ONLY"));
            assert!(!source.contains("unified-test-proxy-root-ca-signing-key"));
        }
        assert!(!normal_dependencies.contains("gmofg-proxy-product-payment"));
    }

    #[test]
    fn isolated_test_profile_preserves_payment_assets_and_labels() {
        let profile = PaymentProductProfile::isolated_test_tool();
        let authority = profile
            .embedded_test_authority()
            .expect("explicit test-only signing authority");

        assert_eq!(
            authority.public_certificate_pem,
            profile.public_root_ca_pem()
        );
        assert!(authority.signing_key_pem.contains("PRIVATE KEY"));
        assert_eq!(authority.required_subject_marker, "TEST ONLY");
        assert!(
            profile
                .bundled_upstream_ca_pem()
                .expect("Payment server certificate")
                .starts_with(b"-----BEGIN CERTIFICATE-----")
        );
        assert!(
            ProductCertificatePolicy::labels(&profile)
                .upstream_bundled_usage
                .contains("Payment server.crt")
        );
        gmofg_proxy_product_api::validate_product_profile(&profile)
            .expect("Payment profile contract");
        assert_eq!(
            profile.storage().database_file_name,
            "gmofg-payment-proxy.sqlite3"
        );
        assert_eq!(profile.channels().len(), 2);
        assert_eq!(profile.channels()[0].id, "transaction");
        assert_eq!(profile.channels()[1].listen_port, 16_127);
        assert_eq!(
            profile
                .request_classifier()
                .classify(ProductMessageContext {
                    channel_id: "dll",
                    start_line: b"POST / HTTP/1.1",
                    headers: &[],
                    body: br#"{"TransactionType":"D48","RequestID":"2740072778"}"#,
                })
                .request_type
                .as_deref(),
            Some("D48")
        );
        let encoded = profile
            .body_codec()
            .encode("決済OK")
            .expect("Payment Shift-JIS");
        assert_eq!(
            profile.body_codec().decode(&encoded).expect("decode"),
            "決済OK"
        );
        assert_eq!(
            profile
                .body_codec()
                .encode("🧪")
                .expect_err("emoji is not representable")
                .code,
            "SHIFT_JIS_ENCODE_FAILED"
        );
        assert_eq!(
            profile
                .body_codec()
                .decode(&[0x81])
                .expect_err("truncated lead byte is invalid")
                .code,
            "SHIFT_JIS_DECODE_FAILED"
        );
    }
}
