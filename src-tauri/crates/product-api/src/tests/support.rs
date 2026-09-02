use std::sync::Arc;

use crate::{
    BodyCodec, CertificateLabels, ClassifiedRequest, ProductCertificatePolicy, ProductChannel,
    ProductError, ProductFaultTemplate, ProductLabels, ProductMessageContext, ProductProfile,
    ProductStorageNamespace, RequestClassifier,
};

#[derive(Debug)]
pub(super) struct TestProfile {
    pub channels: &'static [ProductChannel],
    pub storage: ProductStorageNamespace,
    pub faults: &'static [ProductFaultTemplate],
}

#[derive(Debug)]
struct Utf8Codec;

#[derive(Debug)]
struct EmptyClassifier;

pub(super) const VALID_CHANNELS: &[ProductChannel] = &[
    channel("alpha_2.v1", 20_001, "https://alpha.example.test"),
    channel("A-channel", 20_002, "https://beta.example.test"),
];
pub(super) const DUPLICATE_PORTS: &[ProductChannel] = &[
    channel("alpha", 20_001, "https://alpha.example.test"),
    channel("beta", 20_001, "https://beta.example.test"),
];
pub(super) const INVALID_ID: &[ProductChannel] =
    &[channel("-alpha", 20_001, "https://alpha.example.test")];
pub(super) const INVALID_URL: &[ProductChannel] = &[channel("alpha", 20_001, "https://:bad")];
pub(super) const VALID_FAULTS: &[ProductFaultTemplate] = &[fault("request_delay", "alpha_2.v1")];
pub(super) const UNKNOWN_CHANNEL_FAULTS: &[ProductFaultTemplate] =
    &[fault("request_delay", "missing")];
pub(super) const DUPLICATE_FAULTS: &[ProductFaultTemplate] = &[
    fault("request_delay", "alpha_2.v1"),
    fault("request_delay", "A-channel"),
];
pub(super) const UNKNOWN_CAPABILITY: &[ProductFaultTemplate] =
    &[fault("not-supported", "alpha_2.v1")];

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

pub(super) const fn storage() -> ProductStorageNamespace {
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
