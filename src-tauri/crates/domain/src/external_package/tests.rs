use super::*;
use crate::{DocumentFieldType, DocumentValue, ErrorCode};
use serde_json::{Value, json};

mod registration_coverage;
mod runtime_coverage;

fn valid_registration_json() -> Value {
    json!({
        "api": 1,
        "package": {
            "id": "vendor-dukpt-iso8583",
            "name": "DUKPT ISO8583",
            "version": "1.0.0",
            "description": "使用外部密码设备处理 DUKPT 报文"
        },
        "document": {
            "upstream": {
                "schema": {
                    "id": "dukpt-iso8583-upstream",
                    "title": "DUKPT ISO8583 Upstream",
                    "version": 1,
                    "fields": [
                        {"name": "message_type", "label": "MTI", "type": "string"},
                        {"name": "amount", "label": "Amount", "type": "int"},
                        {"name": "approved", "label": "Approved", "type": "bool"},
                        {"name": "icc_data", "label": "ICC Data", "type": "blob"}
                    ]
                },
                "display": "render_message"
            },
            "downstream": {
                "schema": {
                    "id": "dukpt-iso8583-downstream",
                    "title": "DUKPT ISO8583 Downstream",
                    "version": 1,
                    "fields": [
                        {"name": "message_type", "label": "MTI", "type": "string"},
                        {"name": "response_code", "label": "Response Code", "type": "string"}
                    ]
                },
                "display": "render_message"
            }
        },
        "hooks": {
            "upstream": {
                "frame": "split_frame",
                "decode": "decrypt_and_decode",
                "encode": "encode_and_encrypt"
            },
            "downstream": {
                "frame": "split_frame",
                "decode": "decrypt_and_decode",
                "encode": "encode_and_encrypt"
            }
        }
    })
}

fn registration() -> ExternalPackageRegistration {
    serde_json::from_value(valid_registration_json()).expect("valid external package registration")
}

#[test]
fn registration_accepts_the_current_strict_contract_and_maps_namespaces() {
    let registration = registration();

    assert_eq!(registration.api(), EXTERNAL_PACKAGE_API_V1);
    assert_eq!(
        registration.package().identity().id.as_str(),
        "vendor-dukpt-iso8583"
    );
    assert_eq!(registration.package().identity().version.as_str(), "1.0.0");
    assert_eq!(registration.package().name(), "DUKPT ISO8583");
    assert_eq!(
        registration.document().upstream().schema().fields().len(),
        4
    );
    assert_eq!(
        registration.hooks().upstream().frame().qualified(
            ExternalPackageMethodNamespace::Hooks,
            ExternalPackageDirection::Upstream,
        ),
        "hooks.upstream.split_frame"
    );
    assert_eq!(
        registration.document().downstream().display().qualified(
            ExternalPackageMethodNamespace::Document,
            ExternalPackageDirection::Downstream,
        ),
        "document.downstream.render_message"
    );
}

#[test]
fn registration_rejects_unknown_keys_at_every_object_level() {
    let paths = [
        vec!["extra"],
        vec!["package", "extra"],
        vec!["document", "extra"],
        vec!["document", "upstream", "extra"],
        vec!["hooks", "extra"],
        vec!["hooks", "upstream", "extra"],
    ];

    for path in paths {
        let mut value = valid_registration_json();
        let (last, parents) = path.split_last().expect("path is non-empty");
        let mut object = &mut value;
        for parent in parents {
            object = &mut object[parent];
        }
        object[*last] = json!(true);
        assert!(
            serde_json::from_value::<ExternalPackageRegistration>(value).is_err(),
            "unknown key at {path:?} must fail"
        );
    }
}

#[test]
fn registration_rejects_unsupported_api_empty_metadata_and_invalid_schema() {
    for (pointer, invalid) in [
        ("/api", json!(2)),
        ("/package/id", json!("")),
        ("/package/name", json!(" \n")),
        ("/package/version", json!("")),
        ("/document/upstream/schema/fields", json!([])),
    ] {
        let mut value = valid_registration_json();
        *value.pointer_mut(pointer).expect("test pointer exists") = invalid;
        assert!(
            serde_json::from_value::<ExternalPackageRegistration>(value).is_err(),
            "invalid value at {pointer} must fail"
        );
    }

    let mut duplicate = valid_registration_json();
    duplicate["document"]["upstream"]["schema"]["fields"] = json!([
        {"name": "amount", "label": "Amount", "type": "int"},
        {"name": "amount", "label": "Duplicate", "type": "string"}
    ]);
    assert!(serde_json::from_value::<ExternalPackageRegistration>(duplicate).is_err());

    let mut unknown_field_type = valid_registration_json();
    unknown_field_type["document"]["upstream"]["schema"]["fields"][0]["type"] = json!("decimal");
    assert!(serde_json::from_value::<ExternalPackageRegistration>(unknown_field_type).is_err());

    let mut missing_method = valid_registration_json();
    missing_method["hooks"]["upstream"]
        .as_object_mut()
        .unwrap()
        .remove("decode");
    assert!(serde_json::from_value::<ExternalPackageRegistration>(missing_method).is_err());
}

#[test]
fn method_suffix_rejects_dots_invalid_identifiers_and_same_namespace_conflicts() {
    for suffix in ["", ".", "split.frame", "1frame", "frame-name", "报文"] {
        assert!(
            ExternalPackageMethodSuffix::new(suffix).is_err(),
            "{suffix}"
        );
    }

    let mut conflict = valid_registration_json();
    conflict["hooks"]["upstream"]["encode"] = json!("decrypt_and_decode");
    assert!(serde_json::from_value::<ExternalPackageRegistration>(conflict).is_err());

    // 不同方向和 document/hooks 命名空间可以安全复用相同后缀。
    let mut reusable = valid_registration_json();
    reusable["document"]["upstream"]["display"] = json!("split_frame");
    reusable["document"]["downstream"]["display"] = json!("split_frame");
    assert!(serde_json::from_value::<ExternalPackageRegistration>(reusable).is_ok());
}

#[test]
fn external_document_wire_round_trips_all_types_and_preserves_unset_fields() {
    let registration = registration();
    let schema = registration.document().upstream().schema();
    let wire: ExternalDocumentWire = serde_json::from_value(json!({
        "message_type": {"type": "string", "value": "0200"},
        "amount": {"type": "int", "value": "1000"},
        "approved": {"type": "bool", "value": false},
        "icc_data": {"type": "blob", "value_base64": "n6c="}
    }))
    .unwrap();

    let document = wire.into_document(schema).unwrap();
    assert_eq!(
        document.get("message_type").unwrap(),
        &DocumentValue::String("0200".into())
    );
    assert_eq!(document.get("amount").unwrap(), &DocumentValue::Int(1000));
    assert_eq!(
        document.get("approved").unwrap(),
        &DocumentValue::Bool(false)
    );
    assert_eq!(
        document.get("icc_data").unwrap(),
        &DocumentValue::Blob(vec![0x9f, 0xa7])
    );

    let encoded = serde_json::to_value(ExternalDocumentWire::from_document(&document)).unwrap();
    assert_eq!(encoded["amount"], json!({"type": "int", "value": "1000"}));
    assert_eq!(
        encoded["icc_data"],
        json!({"type": "blob", "value_base64": "n6c="})
    );

    let empty = ExternalDocumentWire::default()
        .into_document(schema)
        .unwrap();
    assert!(!empty.has("amount").unwrap());
}

#[test]
fn external_document_wire_json_contract_is_frozen() {
    let schema = registration().document().upstream().schema().clone();
    let empty = ExternalDocumentWire::default();
    assert_eq!(serde_json::to_string(&empty).unwrap(), "{}");

    let golden = concat!(
        r#"{"amount":{"type":"int","value":"-9223372036854775808"},"#,
        r#""icc_data":{"type":"blob","value_base64":""},"#,
        r#""message_type":{"type":"string","value":""}}"#,
    );
    let wire: ExternalDocumentWire = serde_json::from_str(golden).unwrap();
    assert_eq!(serde_json::to_string(&wire).unwrap(), golden);

    let document = wire.into_document(&schema).unwrap();
    assert!(!document.has("approved").unwrap());
    assert_eq!(
        document.get("amount").unwrap(),
        &DocumentValue::Int(i64::MIN)
    );
    assert_eq!(
        document.get("icc_data").unwrap(),
        &DocumentValue::Blob(Vec::new())
    );
    assert_eq!(
        document.get("message_type").unwrap(),
        &DocumentValue::String(String::new())
    );
    assert_eq!(
        serde_json::to_string(&ExternalDocumentWire::from_document(&document)).unwrap(),
        golden
    );
}

#[test]
fn external_document_wire_rejects_unknown_fields_type_mismatch_and_noncanonical_values() {
    let registration = registration();
    let schema = registration.document().upstream().schema();
    let cases = [
        json!({"unknown": {"type": "string", "value": "x"}}),
        json!({"amount": {"type": "string", "value": "1000"}}),
        json!({"amount": {"type": "int", "value": "9223372036854775808"}}),
        json!({"amount": {"type": "int", "value": "+1"}}),
        json!({"icc_data": {"type": "blob", "value_base64": "n6c"}}),
        json!({"icc_data": {"type": "blob", "value_base64": "***="}}),
        json!({"approved": {"type": "bool", "value": "false"}}),
        json!({"amount": {"type": "int", "value": "1", "extra": true}}),
    ];

    for value in cases {
        if let Ok(wire) = serde_json::from_value::<ExternalDocumentWire>(value) {
            assert!(wire.into_document(schema).is_err());
        }
    }

    assert!(
        serde_json::from_str::<ExternalDocumentWire>(
            r#"{"amount":{"type":"int","value":"1"},"amount":{"type":"int","value":"2"}}"#,
        )
        .is_err()
    );
}

#[test]
fn external_document_wire_rejects_non_object_values() {
    assert!(serde_json::from_value::<ExternalDocumentWire>(json!([])).is_err());
}

#[test]
fn external_document_int_wire_accepts_exact_i64_boundaries() {
    let registration = registration();
    let schema = registration.document().upstream().schema();
    for value in [i64::MIN, i64::MAX] {
        let wire: ExternalDocumentWire = serde_json::from_value(json!({
            "amount": {"type": "int", "value": value.to_string()}
        }))
        .unwrap();
        let document = wire.into_document(schema).unwrap();
        assert_eq!(document.get("amount").unwrap(), &DocumentValue::Int(value));
    }
}

#[test]
fn frame_result_is_a_strict_closed_union() {
    assert_eq!(
        serde_json::from_value::<ExternalFrameResult>(json!({"status": "need_more"})).unwrap(),
        ExternalFrameResult::NeedMore
    );
    assert_eq!(
        serde_json::from_value::<ExternalFrameResult>(json!({
            "status": "complete",
            "consumed_bytes": 59
        }))
        .unwrap(),
        ExternalFrameResult::Complete { consumed_bytes: 59 }
    );
    for invalid in [
        json!({"status": "unknown"}),
        json!({"status": "need_more", "consumed_bytes": 1}),
        json!({"status": "complete"}),
        json!({"status": "complete", "consumed_bytes": 0}),
        json!({"status": "complete", "consumed_bytes": 1, "extra": true}),
    ] {
        assert!(serde_json::from_value::<ExternalFrameResult>(invalid).is_err());
    }
}

#[test]
fn runtime_wire_dtos_encode_bytes_canonically_and_reject_unknown_keys() {
    let frame = ExternalFrameRequest::from_bytes(&[0x00, 0xff]);
    assert_eq!(
        serde_json::to_value(&frame).unwrap(),
        json!({"buffer_base64": "AP8="})
    );
    assert_eq!(frame.bytes().unwrap(), vec![0x00, 0xff]);

    let encoded: ExternalEncodeResponse =
        serde_json::from_value(json!({"frame_base64": "AP8="})).unwrap();
    assert_eq!(encoded.bytes().unwrap(), vec![0x00, 0xff]);
    let noncanonical: ExternalEncodeResponse =
        serde_json::from_value(json!({"frame_base64": "AP8"})).unwrap();
    assert!(noncanonical.bytes().is_err());
    assert!(
        serde_json::from_value::<ExternalEncodeResponse>(json!({
            "frame_base64": "AP8", "extra": true
        }))
        .is_err()
    );
}

#[test]
fn document_type_mismatch_uses_the_existing_stable_domain_error() {
    let registration = registration();
    let schema = registration.document().upstream().schema();
    let wire: ExternalDocumentWire = serde_json::from_value(json!({
        "amount": {"type": "string", "value": "1000"}
    }))
    .unwrap();
    let error = wire.into_document(schema).unwrap_err();
    assert_eq!(error.code, ErrorCode::DocumentFieldTypeMismatch);
    assert_eq!(error.code.as_str(), "DOCUMENT_FIELD_TYPE_MISMATCH");
}

#[test]
fn schema_field_types_remain_the_shared_domain_contract() {
    let registration = registration();
    assert_eq!(
        registration.document().upstream().schema().fields()[1].field_type(),
        DocumentFieldType::Int
    );
}
