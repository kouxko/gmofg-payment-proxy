use super::*;

#[test]
fn socket_capture_identifiers_round_trip_uuid_and_formatting() {
    let uuid = Uuid::from_u128(42);
    let capture = SocketCaptureId::from_uuid(uuid);
    let exchange = SocketExchangeId::from_uuid(uuid);
    let connection = SocketConnectionId::from_uuid(uuid);

    assert_eq!(capture.as_uuid(), uuid);
    assert_eq!(exchange.as_uuid(), uuid);
    assert_eq!(connection.as_uuid(), uuid);
    assert_eq!(capture.to_string(), uuid.to_string());
    assert_eq!(exchange.to_string(), uuid.to_string());
    assert_eq!(connection.to_string(), uuid.to_string());
    assert_eq!(format!("{capture:?}"), uuid.to_string());
    assert_eq!(format!("{exchange:?}"), uuid.to_string());
    assert_eq!(format!("{connection:?}"), uuid.to_string());
}

#[test]
fn socket_capture_identifier_defaults_generate_non_nil_values() {
    assert_ne!(SocketCaptureId::default().as_uuid(), Uuid::nil());
    assert_ne!(SocketExchangeId::default().as_uuid(), Uuid::nil());
    assert_ne!(SocketConnectionId::default().as_uuid(), Uuid::nil());
}

#[test]
fn display_results_round_trip_and_redact_payloads_from_debug() {
    let html = SocketDisplayResult::UntrustedHtml {
        html: "<p>secret</p>".into(),
    };
    let fallback = SocketDisplayResult::HexFallback {
        reason: SocketDisplayFallbackReason::EntryPointFailed,
        diagnostic: Some(SocketDisplayDiagnostic {
            code: "DISPLAY_ENTRY_FAILED".into(),
            message: "secret diagnostic".into(),
            external_package_call: None,
        }),
    };

    for value in [html, fallback] {
        let wire = serde_json::to_value(&value).unwrap();
        let restored = serde_json::from_value::<SocketDisplayResult>(wire).unwrap();
        assert_eq!(restored, value);
        let debug = format!("{value:?}");
        assert!(!debug.contains("secret"));
    }
}

#[test]
fn capture_document_preserves_every_value_type_and_sparse_slots() {
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("all-types").unwrap(),
        3,
        "All types",
        vec![
            field("text", DocumentFieldType::String),
            field("count", DocumentFieldType::Int),
            field("approved", DocumentFieldType::Bool),
            field("raw", DocumentFieldType::Blob),
            field("unset", DocumentFieldType::String),
        ],
    )
    .unwrap();
    let mut source = Document::new(schema);
    source
        .set("text", DocumentValue::String("value".into()))
        .unwrap();
    source.set("count", DocumentValue::Int(i64::MIN)).unwrap();
    source.set("approved", DocumentValue::Bool(true)).unwrap();
    source
        .set("raw", DocumentValue::Blob(vec![0, 255]))
        .unwrap();

    let capture = SocketCaptureDocument::from_document(&source);

    assert_eq!(
        capture.get("text"),
        Some(&SocketCaptureDocumentValue::String("value".into()))
    );
    assert_eq!(
        capture.get("count").unwrap().to_string_for_test(),
        i64::MIN.to_string()
    );
    assert_eq!(
        capture.get("approved"),
        Some(&SocketCaptureDocumentValue::Bool(true))
    );
    assert_eq!(
        capture.get("raw"),
        Some(&SocketCaptureDocumentValue::Blob(vec![0, 255]))
    );
    assert_eq!(capture.get("unset"), None);
    assert_eq!(capture.get("missing"), None);
    assert!(!format!("{capture:?}").contains("value"));
    let restored =
        serde_json::from_value::<SocketCaptureDocument>(serde_json::to_value(&capture).unwrap())
            .unwrap();
    assert_eq!(restored, capture);
    for value in restored.values.iter().flatten() {
        assert!(!format!("{value:?}").contains("value"));
    }
}

#[test]
fn both_capture_payload_variants_round_trip_and_account_logical_bytes() {
    let relay_payload = SocketCapturePayload::RelayFrame(Box::new(relay()));
    let response = document();
    let local_payload = SocketCapturePayload::LocalExchange(Box::new(SocketLocalExchangeCapture {
        exchange_id: SocketExchangeId::new(),
        package: package(),
        request_schema: schema_ref(),
        response_schema: schema_ref(),
        request_origin: b"0200".to_vec(),
        request_document: SocketCaptureDocument::from_document(&response),
        request_display: display(),
        response_document: SocketCaptureDocument::from_document(&response),
        matched_request_rule_ids: Vec::new(),
        matched_response_rule_ids: Vec::new(),
        written_response: b"0200".to_vec(),
        response_display: display(),
    }));

    for payload in [relay_payload, local_payload] {
        let logical_bytes = payload.logical_bytes();
        assert!(logical_bytes > SocketCaptureRecord::ENTITY_FIXED_OVERHEAD_BYTES);
        let restored =
            serde_json::from_value::<SocketCapturePayload>(serde_json::to_value(&payload).unwrap())
                .unwrap();
        assert_eq!(restored, payload);
        let debug = format!("{payload:?}");
        for secret in [
            "origin:",
            "written:",
            "request_origin:",
            "request_document:",
            "response_document:",
            "written_response:",
            "html:",
        ] {
            assert!(!debug.contains(secret), "Debug leaked {secret}: {debug}");
        }
    }
}

fn field(name: &str, field_type: DocumentFieldType) -> DocumentField {
    DocumentField::new(DocumentFieldName::new(name).unwrap(), field_type, name).unwrap()
}

trait IntegerTextForTest {
    fn to_string_for_test(&self) -> String;
}

impl IntegerTextForTest for SocketCaptureDocumentValue {
    fn to_string_for_test(&self) -> String {
        let SocketCaptureDocumentValue::Int(value) = self else {
            panic!("expected integer capture value")
        };
        value.as_str().to_owned()
    }
}
