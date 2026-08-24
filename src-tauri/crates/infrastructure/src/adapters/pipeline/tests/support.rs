use std::{
    net::SocketAddr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering},
    time::SystemTime,
};

use intercept_proxy_application::{
    RawHttpHeaderViewModel, RuleDraft as AppRuleDraft, RuleSummaryViewModel, RuleViewModel,
};
use intercept_proxy_domain::DropResponseMode;
use intercept_proxy_product_api::ProductMessageContext;
use intercept_proxy_runtime::{RawHeader, TlsPeerIdentity};
use serde_json::json;

use super::*;
fn test_capture_repository() -> Arc<CaptureRepositoryAdapter> {
    Arc::new(CaptureRepositoryAdapter::new(Arc::new(
        InMemorySessionStore::default(),
    )))
}

#[derive(Debug)]
struct Utf8BodyCodec;

impl BodyCodec for Utf8BodyCodec {
    fn id(&self) -> &'static str {
        "test-utf8"
    }

    fn name(&self) -> &'static str {
        "Test UTF-8"
    }

    fn decode(&self, bytes: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| {
                intercept_proxy_product_api::ProductError::new(
                    "BODY_DECODE_FAILED",
                    error.to_string(),
                )
            })
    }

    fn encode(&self, text: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        Ok(text.as_bytes().to_vec())
    }
}

fn test_body_codec() -> Arc<dyn BodyCodec> {
    Arc::new(Utf8BodyCodec)
}

#[derive(Debug)]
struct TestRequestClassifier;

impl RequestClassifier for TestRequestClassifier {
    fn classify(
        &self,
        message: ProductMessageContext<'_>,
    ) -> intercept_proxy_product_api::ClassifiedRequest {
        let request_id = message
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(b"x-test-request-id"))
            .map(|header| String::from_utf8_lossy(header.value).into_owned());
        intercept_proxy_product_api::ClassifiedRequest {
            request_id,
            request_type: None,
        }
    }
}

fn test_request_classifier() -> Arc<dyn RequestClassifier> {
    Arc::new(TestRequestClassifier)
}

fn test_channel_labels() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("transaction".into(), "交易".into()),
        ("dll".into(), "DLL".into()),
        ("alpha".into(), "Alpha".into()),
    ])
}

fn test_product_hooks() -> RuntimePipelineProductHooks {
    RuntimePipelineProductHooks {
        body_codec: test_body_codec(),
        request_classifier: test_request_classifier(),
        channel_labels: test_channel_labels(),
    }
}

#[derive(Debug)]
struct StableErrorCodec;

impl BodyCodec for StableErrorCodec {
    fn id(&self) -> &'static str {
        "stable-error"
    }

    fn name(&self) -> &'static str {
        "Stable Error"
    }

    fn decode(&self, _bytes: &[u8]) -> Result<String, intercept_proxy_product_api::ProductError> {
        Err(intercept_proxy_product_api::ProductError::new(
            "PRODUCT_DECODE_FAILED",
            "decode failed",
        ))
    }

    fn encode(&self, _text: &str) -> Result<Vec<u8>, intercept_proxy_product_api::ProductError> {
        Err(intercept_proxy_product_api::ProductError::new(
            "PRODUCT_ENCODE_FAILED",
            "encode failed",
        ))
    }
}
