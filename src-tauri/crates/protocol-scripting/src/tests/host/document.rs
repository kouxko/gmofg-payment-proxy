use intercept_proxy_domain::{Document, DocumentValue};
use rhai::{Array, Blob, Dynamic, ImmutableString, Map};

use super::common::{engine, host};

#[test]
fn document_create_is_bound_to_the_package_schema_and_starts_empty() {
    let host = host();
    let document = host.create_document();
    assert_eq!(document.schema().id().as_str(), "host-test-message");
    assert_eq!(document.schema().version(), 3);
    assert!(document.fields().all(|state| state.value.is_none()));

    let from_rhai = engine().eval::<Document>("document::create()").unwrap();
    assert_eq!(from_rhai.schema(), document.schema());
    assert!(from_rhai.fields().all(|state| state.value.is_none()));
}

#[test]
fn all_four_document_types_round_trip_without_implicit_conversion() {
    let document = engine()
        .eval::<Document>(
            r#"
                let value = document::create();
                value.set("text_value", "007");
                value.set("int_value", 7);
                value.set("bool_value", true);
                value.set("blob_value", blob(3, 0xff));
                value
            "#,
        )
        .unwrap();

    assert_eq!(
        document.get("text_value").unwrap(),
        &DocumentValue::String("007".to_owned())
    );
    assert_eq!(document.get("int_value").unwrap(), &DocumentValue::Int(7));
    assert_eq!(
        document.get("bool_value").unwrap(),
        &DocumentValue::Bool(true)
    );
    assert_eq!(
        document.get("blob_value").unwrap(),
        &DocumentValue::Blob(vec![255, 255, 255])
    );

    let values = engine()
        .eval::<Array>(
            r#"
                let value = document::create();
                value.set("text_value", "007");
                value.set("int_value", 7);
                value.set("bool_value", true);
                value.set("blob_value", blob(2, 0xaa));
                [
                    value.get("text_value"),
                    value.get("int_value"),
                    value.get("bool_value"),
                    value.get("blob_value")
                ]
            "#,
        )
        .unwrap();
    assert_eq!(values[0].clone_cast::<ImmutableString>().as_str(), "007");
    assert_eq!(values[1].as_int().unwrap(), 7);
    assert!(values[2].as_bool().unwrap());
    assert_eq!(values[3].clone_cast::<Blob>(), vec![0xaa, 0xaa]);
}

#[test]
fn host_int_is_always_signed_64_bit_and_preserves_its_boundaries() {
    assert_eq!(std::mem::size_of::<rhai::INT>(), 8);
    let document = engine()
        .eval::<Document>(
            r#"
                let value = document::create();
                value.set("int_value", 9223372036854775807);
                value
            "#,
        )
        .unwrap();
    assert_eq!(
        document.get("int_value").unwrap(),
        &DocumentValue::Int(i64::MAX)
    );
}

#[test]
fn has_get_and_set_return_stable_safe_errors_for_invalid_access() {
    for (script, code) in [
        (
            r#"let d = document::create(); d.has("not_a_secret_field")"#,
            "DOCUMENT_FIELD_UNDECLARED",
        ),
        (
            r#"let d = document::create(); d.get("not_a_secret_field")"#,
            "DOCUMENT_FIELD_UNDECLARED",
        ),
        (
            r#"let d = document::create(); d.set("not_a_secret_field", 7)"#,
            "DOCUMENT_FIELD_UNDECLARED",
        ),
        (
            r#"let d = document::create(); d.get("text_value")"#,
            "DOCUMENT_FIELD_UNASSIGNED",
        ),
        (
            r#"let d = document::create(); d.set("int_value", "7")"#,
            "DOCUMENT_FIELD_TYPE_MISMATCH",
        ),
    ] {
        let error = engine().eval::<Dynamic>(script).unwrap_err().to_string();
        assert!(error.contains(code));
        assert!(!error.contains("not_a_secret_field"));
    }
}

#[test]
fn has_changes_from_false_to_true_and_failed_set_keeps_the_previous_value() {
    let mut document = host().create_document();
    let mut scope = rhai::Scope::new();
    scope.push("document", document.clone());
    let states = engine()
        .eval_with_scope::<Array>(
            &mut scope,
            r#"
                let before = document.has("int_value");
                document.set("int_value", 7);
                [before, document.has("int_value")]
            "#,
        )
        .unwrap();
    assert!(!states[0].as_bool().unwrap());
    assert!(states[1].as_bool().unwrap());

    document = scope.get_value("document").unwrap();
    let mut failed_scope = rhai::Scope::new();
    failed_scope.push("document", document);
    let error = engine()
        .eval_with_scope::<Dynamic>(&mut failed_scope, r#"document.set("int_value", "wrong")"#)
        .unwrap_err()
        .to_string();
    assert!(error.contains("DOCUMENT_FIELD_TYPE_MISMATCH"));
    let unchanged = failed_scope.get_value::<Document>("document").unwrap();
    assert_eq!(unchanged.get("int_value").unwrap(), &DocumentValue::Int(7));
}

#[test]
fn each_schema_type_rejects_the_other_rhai_value_kinds() {
    for script in [
        r#"let d = document::create(); d.set("text_value", 1)"#,
        r#"let d = document::create(); d.set("int_value", false)"#,
        r#"let d = document::create(); d.set("bool_value", "true")"#,
        r#"let d = document::create(); d.set("blob_value", [1, 2])"#,
    ] {
        let error = engine().eval::<Dynamic>(script).unwrap_err().to_string();
        assert!(error.contains("DOCUMENT_FIELD_TYPE_MISMATCH"));
    }
}

#[test]
fn fields_preserve_schema_order_and_expose_present_value_metadata() {
    let rows = engine()
        .eval::<Array>(
            r#"
                let d = document::create();
                d.set("int_value", 42);
                d.fields()
            "#,
        )
        .unwrap();
    assert_eq!(rows.len(), 4);

    let names = rows
        .iter()
        .map(|row| {
            row.clone_cast::<Map>()["name"]
                .clone_cast::<ImmutableString>()
                .into_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        ["text_value", "int_value", "bool_value", "blob_value"]
    );

    let missing = rows[0].clone_cast::<Map>();
    assert!(!missing["present"].as_bool().unwrap());
    assert!(missing["value"].is_unit());
    assert_eq!(missing["label"].clone_cast::<ImmutableString>(), "Text");
    assert_eq!(missing["type"].clone_cast::<ImmutableString>(), "string");

    let present = rows[1].clone_cast::<Map>();
    assert!(present["present"].as_bool().unwrap());
    assert_eq!(present["value"].as_int().unwrap(), 42);
}

#[test]
fn script_cannot_choose_a_schema_or_store_host_objects_in_document() {
    let create_error = engine()
        .eval::<Dynamic>(r#"document::create("another-schema")"#)
        .unwrap_err()
        .to_string();
    assert!(create_error.contains("create"));

    let mut scope = rhai::Scope::new();
    scope.push(
        "host_value",
        crate::host::context::ProtocolCallContext::new(
            crate::host::context::ProtocolDirection::Upstream,
            crate::host::context::ProtocolStage::Receive,
            "connection-1",
            "listener-1",
        ),
    );
    let error = engine()
        .eval_with_scope::<Dynamic>(
            &mut scope,
            r#"let d = document::create(); d.set("blob_value", host_value)"#,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("DOCUMENT_FIELD_TYPE_MISMATCH"));
}
