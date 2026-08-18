use std::sync::Arc;

use bytes::Bytes;
use intercept_proxy_application::{HttpProtocolBodyViewModel, HttpProtocolDisplayViewModel};
use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
    DocumentSchemaId, DocumentValue, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use intercept_proxy_protocol_scripting::ProtocolDirection;
use intercept_proxy_runtime::PipelinePorts;
use uuid::Uuid;

use crate::adapters::listener_runtime::HttpProtocolObservationSink;

use super::{adapter, request_message, response_message, test_context, transaction_channel};

#[tokio::test]
async fn request_observation_persists_final_body_and_protocol_evidence_together() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(
        Uuid::from_u128(21),
        Uuid::from_u128(22),
        transaction_channel(),
    );
    pipeline.connection_opened(&context).await;
    let mut request = request_message("original");
    pipeline.request(&context, &mut request).await.unwrap();
    request.replace_body(Bytes::from_static(b"request-final"));
    let observation = observation("request_value");

    pipeline
        .record_http_protocol_observation(
            &context,
            ProtocolDirection::Upstream,
            &request,
            observation.clone(),
        )
        .unwrap();

    let recorded = active_record(&pipeline, &context);
    let captured_content = recorded.detail.request.expect("request content");
    assert_eq!(captured_content.body_bytes, b"request-final");
    assert_eq!(captured_content.content_length, b"request-final".len());
    assert_eq!(captured_content.protocol, Some(observation));
    assert_eq!(
        recorded.detail.summary.request_size_bytes,
        b"request-final".len() as u64
    );
}

#[tokio::test]
async fn response_observation_persists_final_body_status_and_protocol_evidence_together() {
    let pipeline = adapter(Vec::new(), 10);
    let context = test_context(
        Uuid::from_u128(31),
        Uuid::from_u128(32),
        transaction_channel(),
    );
    pipeline.connection_opened(&context).await;
    let mut request = request_message("original");
    pipeline.request(&context, &mut request).await.unwrap();
    let mut response = response_message();
    pipeline.response(&context, &mut response).await.unwrap();
    response.replace_body(Bytes::from_static(b"response-final"));
    let observation = observation("response_value");

    pipeline
        .record_http_protocol_observation(
            &context,
            ProtocolDirection::Downstream,
            &response,
            observation.clone(),
        )
        .unwrap();

    let recorded = active_record(&pipeline, &context);
    let captured_content = recorded.detail.response.expect("response content");
    assert_eq!(captured_content.body_bytes, b"response-final");
    assert_eq!(captured_content.content_length, b"response-final".len());
    assert_eq!(captured_content.protocol, Some(observation));
    assert_eq!(recorded.detail.summary.http_status, Some(200));
    assert_eq!(
        recorded.detail.summary.response_size_bytes,
        b"response-final".len() as u64
    );
}

fn active_record(
    pipeline: &super::RuntimePipelineAdapter,
    context: &intercept_proxy_runtime::ConnectionContext,
) -> intercept_proxy_application::SessionRecord {
    let session_id = pipeline
        .state
        .lock()
        .connection(context)
        .and_then(|connection| connection.session_id)
        .expect("active session");
    pipeline.sessions.get_record(session_id).expect("session")
}

fn observation(value: &str) -> HttpProtocolBodyViewModel {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("http-observation-test").unwrap(),
        1,
        "HTTP observation test",
        vec![
            DocumentField::new(
                DocumentFieldName::new("value").unwrap(),
                DocumentFieldType::String,
                "Value",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut document = Document::new(Arc::new(schema));
    document
        .set("value", DocumentValue::String(value.into()))
        .unwrap();
    HttpProtocolBodyViewModel {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("http-observation-test").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        origin_body: value.as_bytes().to_vec(),
        origin_text: value.to_owned(),
        written_body: value.as_bytes().to_vec(),
        written_text: value.to_owned(),
        document,
        stages: Vec::new(),
        display: HttpProtocolDisplayViewModel::UntrustedHtml {
            html: format!("<p>{value}</p>"),
        },
    }
}
