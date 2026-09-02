use std::sync::Arc;

use crate::{
    BodyCodec, CertificateLabels, ClassifiedRequest, ProductCertificatePolicy, ProductChannel,
    ProductError, ProductFaultTemplate, ProductLabels, ProductMessageContext, ProductProfile,
    ProductStorageNamespace, RequestClassifier,
};

/// Intercept Proxy 的无业务默认配置。
///
/// 可编辑监听器、编码器、提取器与断言由 `domain::ProxyWorkspace` 持有。此配置只为
/// Host 提供稳定的存储、安全命名空间和通用文案。
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
    fn fixed_installation_root_ca_pem(&self) -> Option<&'static [u8]> {
        None
    }

    fn fixed_installation_root_key_pem(&self) -> Option<&'static [u8]> {
        None
    }

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
            already_exists_message: "Root CA 已经初始化。",
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
