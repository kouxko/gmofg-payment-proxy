use chrono::{TimeZone, Utc};
use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
    DocumentSchemaId, DocumentValue, ListenerId, ProtocolPackageId, ProtocolPackageRef,
    ProtocolPackageVersion, SocketDirection, SocketDocumentRuleId, WorkspaceId,
};
use serde_json::json;
use uuid::Uuid;

use super::*;

#[path = "socket_capture_tests/coverage.rs"]
mod coverage;

fn package() -> ProtocolPackageRef {
    ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583").unwrap(),
        version: ProtocolPackageVersion::new("1.2.3").unwrap(),
    }
}

fn document() -> Document {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("payment").unwrap(),
        7,
        "Payment",
        vec![
            DocumentField::new(
                DocumentFieldName::new("mti").unwrap(),
                DocumentFieldType::String,
                "MTI",
            )
            .unwrap(),
            DocumentField::new(
                DocumentFieldName::new("field_39").unwrap(),
                DocumentFieldType::String,
                "Response code",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let mut document = Document::new(schema);
    document
        .set("mti", DocumentValue::String("0200".into()))
        .unwrap();
    document
}

fn schema_ref() -> SocketCaptureSchemaRef {
    SocketCaptureSchemaRef {
        id: DocumentSchemaId::new("payment").unwrap(),
        version: 7,
    }
}

fn display(enabled: bool) -> SocketDisplayResult {
    if enabled {
        SocketDisplayResult::UntrustedHtml {
            html: "<dl><dt>MTI</dt><dd>0200</dd></dl>".into(),
        }
    } else {
        SocketDisplayResult::HexFallback {
            reason: SocketDisplayFallbackReason::EncodeDisabled,
            diagnostic: None,
        }
    }
}

fn relay(decode_enabled: bool, encode_enabled: bool) -> SocketRelayFrameCapture {
    SocketRelayFrameCapture {
        direction: SocketDirection::Upstream,
        package: package(),
        schema: schema_ref(),
        decode_enabled,
        encode_enabled,
        origin: vec![0x02, 0x30, 0x32, 0x30, 0x30, 0x03],
        document: decode_enabled.then(|| SocketCaptureDocument::from_document(&document())),
        matched_rule_ids: decode_enabled
            .then(SocketDocumentRuleId::new)
            .into_iter()
            .collect(),
        written: if encode_enabled {
            vec![0x02, 0x30, 0x32, 0x31, 0x30, 0x03]
        } else {
            vec![0x02, 0x30, 0x32, 0x30, 0x30, 0x03]
        },
        write_kind: if encode_enabled {
            SocketWriteKind::Encoded
        } else {
            SocketWriteKind::Original
        },
        display: display(encode_enabled),
    }
}

fn record(payload: SocketCapturePayload) -> SocketCaptureRecord {
    let connection_id = SocketConnectionId::new();
    SocketCaptureRecord {
        capture_id: SocketCaptureId::new(),
        runtime_epoch: Uuid::new_v4(),
        workspace_id: WorkspaceId::new(),
        listener_id: ListenerId::new(),
        session_id: connection_id.as_uuid(),
        connection_id,
        peer_address: "127.0.0.1:43100".into(),
        occurred_at: Utc.with_ymd_and_hms(2026, 8, 15, 10, 0, 0).unwrap(),
        completed_at: Utc.with_ymd_and_hms(2026, 8, 15, 10, 0, 1).unwrap(),
        payload,
    }
}

#[test]
fn relay_four_processing_states_keep_exact_origin_and_written_evidence() {
    for decode_enabled in [false, true] {
        for encode_enabled in [false, true] {
            let capture = relay(decode_enabled, encode_enabled);
            assert_eq!(capture.document.is_some(), decode_enabled);
            assert_eq!(capture.origin, [0x02, 0x30, 0x32, 0x30, 0x30, 0x03]);
            assert_eq!(
                capture.write_kind,
                if encode_enabled {
                    SocketWriteKind::Encoded
                } else {
                    SocketWriteKind::Original
                }
            );
            if !encode_enabled {
                assert_eq!(capture.written, capture.origin);
                assert!(matches!(
                    capture.display,
                    SocketDisplayResult::HexFallback {
                        reason: SocketDisplayFallbackReason::EncodeDisabled,
                        ..
                    }
                ));
            }
        }
    }
}

#[test]
fn local_exchange_keeps_request_and_response_documents_separate() {
    let request = document();
    let mut response = request.clone();
    response
        .set("mti", DocumentValue::String("0210".into()))
        .unwrap();
    response
        .set("field_39", DocumentValue::String("00".into()))
        .unwrap();
    let request_snapshot = SocketCaptureDocument::from_document(&request);
    let exchange = SocketLocalExchangeCapture {
        exchange_id: SocketExchangeId::new(),
        package: package(),
        schema: schema_ref(),
        request_decode_enabled: true,
        response_encode_enabled: true,
        request_origin: b"0200".to_vec(),
        request_document: Some(SocketCaptureDocument::from_document(&request)),
        response_document: SocketCaptureDocument::from_document(&response),
        matched_downstream_rule_ids: vec![SocketDocumentRuleId::new()],
        written_response: b"021000".to_vec(),
        response_write_kind: SocketWriteKind::Encoded,
        response_display: display(true),
    };

    assert_eq!(exchange.request_document, Some(request_snapshot));
    assert_eq!(
        exchange.response_document.get("mti").unwrap(),
        &SocketCaptureDocumentValue::String("0210".into())
    );
    assert_eq!(exchange.matched_downstream_rule_ids.len(), 1);
}

#[test]
fn socket_capture_wire_is_strict_and_contains_no_http_projection() {
    let capture = record(SocketCapturePayload::RelayFrame(relay(true, true)));
    let value = serde_json::to_value(&capture).unwrap();
    let object = value.as_object().unwrap();
    for forbidden in ["headers", "http_status", "method", "json_path", "status"] {
        assert!(!object.contains_key(forbidden));
        assert!(!value.to_string().contains(&format!("\"{forbidden}\"")));
    }

    let mut unknown_top_level = value.clone();
    unknown_top_level["headers"] = json!({"x-secret": "must reject"});
    assert!(serde_json::from_value::<SocketCaptureRecord>(unknown_top_level).is_err());

    let mut unknown_payload = value;
    unknown_payload["payload"]["capture"]["http_status"] = json!(200);
    assert!(serde_json::from_value::<SocketCaptureRecord>(unknown_payload).is_err());
}

#[test]
fn logical_bytes_count_owned_network_and_display_bytes_exactly() {
    let base = record(SocketCapturePayload::RelayFrame(relay(true, true)));
    let mut larger = base.clone();
    let SocketCapturePayload::RelayFrame(frame) = &mut larger.payload else {
        unreachable!();
    };
    frame.origin.push(0xff);
    frame.written.extend_from_slice(&[0xaa, 0xbb]);
    let SocketDisplayResult::UntrustedHtml { html } = &mut frame.display else {
        unreachable!();
    };
    html.push('中');

    assert_eq!(larger.logical_bytes() - base.logical_bytes(), 1 + 2 + 3);
}

#[test]
fn connection_route_wire_cannot_attach_upstream_to_local_responder() {
    assert!(
        serde_json::from_value::<SocketConnectionRouteViewModel>(json!({
            "topology": "local_responder",
            "configured_address": "forged.example:443"
        }))
        .is_err()
    );
    assert_eq!(
        serde_json::to_value(SocketConnectionRouteViewModel::LocalResponder {
            downstream_tls_peer: None,
        })
        .unwrap(),
        json!({"topology": "local_responder", "downstream_tls_peer": null})
    );
}

#[test]
fn full_capture_debug_reports_shape_without_payload_document_or_html() {
    let detail = SocketCaptureDetailViewModel {
        record: record(SocketCapturePayload::RelayFrame(relay(true, true))),
    };
    let debug = format!("{detail:?}");

    assert!(debug.contains("origin_bytes: 6"));
    assert!(debug.contains("written_bytes: 6"));
    assert!(debug.contains("document_present: true"));
    for secret in ["0200", "<dl>", "field_39", "[2, 48, 50"] {
        assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
    }
}

#[test]
fn display_wire_rejects_unknown_http_fields() {
    assert!(
        serde_json::from_value::<SocketDisplayResult>(json!({
            "type": "untrusted_html",
            "html": "<p>safe later</p>",
            "http_status": 200
        }))
        .is_err()
    );
}

#[test]
fn consistency_rejects_contradictory_relay_facts() {
    let mut capture = record(SocketCapturePayload::RelayFrame(relay(true, true)));
    assert!(capture.is_consistent());

    if let SocketCapturePayload::RelayFrame(frame) = &mut capture.payload {
        frame.decode_enabled = false;
    }
    assert!(!capture.is_consistent());

    if let SocketCapturePayload::RelayFrame(frame) = &mut capture.payload {
        frame.document = None;
        frame.write_kind = SocketWriteKind::Original;
    }
    assert!(!capture.is_consistent());
}

#[test]
fn consistency_rejects_schema_mismatch_and_duplicate_rule_evidence() {
    let mut capture = record(SocketCapturePayload::RelayFrame(relay(true, true)));
    if let SocketCapturePayload::RelayFrame(frame) = &mut capture.payload {
        frame.schema.version += 1;
    }
    assert!(!capture.is_consistent());

    if let SocketCapturePayload::RelayFrame(frame) = &mut capture.payload {
        frame.schema.version -= 1;
        let duplicate = frame.matched_rule_ids[0];
        frame.matched_rule_ids.push(duplicate);
    }
    assert!(!capture.is_consistent());
}

#[test]
fn consistency_rejects_incomplete_local_exchange_and_invalid_timeline() {
    let request = document();
    let exchange = SocketLocalExchangeCapture {
        exchange_id: SocketExchangeId::new(),
        package: package(),
        schema: schema_ref(),
        request_decode_enabled: true,
        response_encode_enabled: false,
        request_origin: b"0200".to_vec(),
        request_document: Some(SocketCaptureDocument::from_document(&request)),
        response_document: SocketCaptureDocument::from_document(&request),
        matched_downstream_rule_ids: Vec::new(),
        written_response: b"0200".to_vec(),
        response_write_kind: SocketWriteKind::Original,
        response_display: display(false),
    };
    let mut capture = record(SocketCapturePayload::LocalExchange(exchange));
    assert!(capture.is_consistent());

    capture.completed_at = capture.occurred_at - chrono::Duration::milliseconds(1);
    assert!(!capture.is_consistent());
    capture.completed_at = capture.occurred_at;
    let SocketCapturePayload::LocalExchange(exchange) = &mut capture.payload else {
        unreachable!();
    };
    exchange.request_origin.clear();
    assert!(!capture.is_consistent());
}

#[test]
fn capture_integer_wire_preserves_full_i64_and_rejects_noncanonical_text() {
    for value in [
        i64::MIN,
        -9_007_199_254_740_993,
        9_007_199_254_740_993,
        i64::MAX,
    ] {
        let capture = SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(value));
        let wire = serde_json::to_value(&capture).unwrap();
        assert_eq!(wire, json!({"type": "int", "value": value.to_string()}));
        assert_eq!(
            serde_json::from_value::<SocketCaptureDocumentValue>(wire).unwrap(),
            capture
        );
    }

    for invalid in ["01", "+1", " 1", "9223372036854775808"] {
        assert!(
            serde_json::from_value::<SocketCaptureDocumentValue>(json!({
                "type": "int",
                "value": invalid
            }))
            .is_err()
        );
    }
    assert!(
        serde_json::from_value::<SocketCaptureDocumentValue>(json!({
            "type": "int",
            "value": "1",
            "headers": {}
        }))
        .is_err()
    );
}

#[test]
fn consistency_rejects_rules_without_decode_and_incompatible_display() {
    let mut relay = record(SocketCapturePayload::RelayFrame(relay(false, false)));
    let SocketCapturePayload::RelayFrame(frame) = &mut relay.payload else {
        unreachable!();
    };
    frame.matched_rule_ids.push(SocketDocumentRuleId::new());
    assert!(!relay.is_consistent());

    let mut local = record(SocketCapturePayload::LocalExchange(
        SocketLocalExchangeCapture {
            exchange_id: SocketExchangeId::new(),
            package: package(),
            schema: schema_ref(),
            request_decode_enabled: false,
            response_encode_enabled: false,
            request_origin: b"0200".to_vec(),
            request_document: None,
            response_document: SocketCaptureDocument::from_document(&document()),
            matched_downstream_rule_ids: Vec::new(),
            written_response: b"0200".to_vec(),
            response_write_kind: SocketWriteKind::Original,
            response_display: SocketDisplayResult::UntrustedHtml {
                html: "<p>forged</p>".into(),
            },
        },
    ));
    assert!(!local.is_consistent());

    let SocketCapturePayload::LocalExchange(exchange) = &mut local.payload else {
        unreachable!();
    };
    exchange.response_display = display(false);
    exchange.written_response = b"different".to_vec();
    assert!(!local.is_consistent());
}
