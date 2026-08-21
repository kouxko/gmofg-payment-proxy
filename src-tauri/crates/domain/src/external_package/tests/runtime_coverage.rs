use crate::{
    ExternalDecodeRequest, ExternalDecodeResponse, ExternalDisplayRequest, ExternalDisplayResponse,
    ExternalDocumentWire, ExternalEncodeRequest, ExternalEncodeResponse, ExternalFrameRequest,
    ExternalFrameResult,
};
use serde_json::json;

#[test]
fn frame_result_serializes_each_closed_union_variant() {
    assert_eq!(
        serde_json::to_value(ExternalFrameResult::NeedMore).expect("need_more serializes"),
        json!({"status": "need_more"})
    );
    assert_eq!(
        serde_json::to_value(ExternalFrameResult::Complete { consumed_bytes: 7 })
            .expect("complete serializes"),
        json!({"status": "complete", "consumed_bytes": 7})
    );
}

#[test]
fn decode_request_round_trips_canonical_bytes() {
    let request = ExternalDecodeRequest::from_bytes(&[0x00, 0xff, 0x7f]);

    assert_eq!(
        request.bytes().expect("canonical bytes decode"),
        [0x00, 0xff, 0x7f]
    );
    assert_eq!(
        serde_json::to_value(request).expect("request serializes"),
        json!({"frame_base64": "AP9/"})
    );
}

#[test]
fn byte_requests_reject_malformed_and_noncanonical_base64() {
    for value in ["***=", "AP8"] {
        let frame: ExternalFrameRequest =
            serde_json::from_value(json!({"buffer_base64": value})).expect("wire shape is valid");
        let decode: ExternalDecodeRequest =
            serde_json::from_value(json!({"frame_base64": value})).expect("wire shape is valid");

        assert!(frame.bytes().is_err(), "frame value {value} must fail");
        assert!(decode.bytes().is_err(), "decode value {value} must fail");
    }
}

#[test]
fn encode_response_constructor_round_trips_canonical_bytes() {
    let response = ExternalEncodeResponse::from_bytes(&[0xde, 0xad, 0xbe, 0xef]);

    assert_eq!(
        response.bytes().expect("canonical bytes decode"),
        [0xde, 0xad, 0xbe, 0xef]
    );
    assert_eq!(
        serde_json::to_value(response).expect("response serializes"),
        json!({"frame_base64": "3q2+7w=="})
    );
}

#[test]
fn encode_response_rejects_malformed_base64() {
    let response: ExternalEncodeResponse =
        serde_json::from_value(json!({"frame_base64": "***="})).expect("wire shape is valid");

    assert!(response.bytes().is_err());
}

#[test]
fn document_runtime_dtos_serialize_the_same_strict_document_wire() {
    let document: ExternalDocumentWire = serde_json::from_value(json!({
        "message_type": {"type": "string", "value": "0200"}
    }))
    .expect("document wire is valid");
    let values = [
        serde_json::to_value(ExternalDecodeResponse {
            document: document.clone(),
        })
        .expect("decode response serializes"),
        serde_json::to_value(ExternalEncodeRequest {
            document: document.clone(),
        })
        .expect("encode request serializes"),
        serde_json::to_value(ExternalDisplayRequest { document })
            .expect("display request serializes"),
    ];

    for value in values {
        assert_eq!(
            value,
            json!({"document": {"message_type": {"type": "string", "value": "0200"}}})
        );
    }
}

#[test]
fn decode_response_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<ExternalDecodeResponse>(json!({
            "document": {},
            "extra": true
        }))
        .is_err()
    );
}

#[test]
fn encode_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<ExternalEncodeRequest>(json!({
            "document": {},
            "extra": true
        }))
        .is_err()
    );
}

#[test]
fn display_request_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<ExternalDisplayRequest>(json!({
            "document": {},
            "extra": true
        }))
        .is_err()
    );
}

#[test]
fn display_response_round_trips_html() {
    let response: ExternalDisplayResponse =
        serde_json::from_value(json!({"html": "<dl></dl>"})).expect("display response is valid");

    assert_eq!(response.html, "<dl></dl>");
    assert_eq!(
        serde_json::to_value(response).expect("display response serializes"),
        json!({"html": "<dl></dl>"})
    );
}

#[test]
fn display_response_rejects_unknown_fields() {
    assert!(
        serde_json::from_value::<ExternalDisplayResponse>(json!({
            "html": "",
            "extra": true
        }))
        .is_err()
    );
}
