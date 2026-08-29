use std::collections::BTreeMap;

use intercept_proxy_domain::{Document, DocumentValue, ErrorCode, JsonPointer};
use rhai::{Array, Dynamic, ImmutableString, Map};

use super::common::engine;

#[test]
fn recursive_values_round_trip_through_the_current_rhai_host() {
    let document = engine()
        .eval::<Document>(
            r#"
                let value = document::create();
                value.set("/text", "007");
                value.set("/number", 7);
                value.set("/flag", true);
                value.set("/nested", #{ items: [1, 2, 3] });
                value
            "#,
        )
        .unwrap();

    assert_eq!(
        document
            .resolve(&JsonPointer::parse("/text").unwrap())
            .unwrap(),
        &DocumentValue::String("007".to_owned())
    );
    assert_eq!(
        document
            .resolve(&JsonPointer::parse("/number").unwrap())
            .unwrap(),
        &DocumentValue::integer(7).unwrap()
    );
    assert_eq!(
        document
            .resolve(&JsonPointer::parse("/nested/items/2").unwrap())
            .unwrap(),
        &DocumentValue::integer(3).unwrap()
    );

    let values = engine()
        .eval::<Array>(
            r#"
                let value = document::create();
                value.set("/text", "007");
                value.set("/number", 7);
                value.set("/flag", true);
                [value.get("/text"), value.get("/number"), value.get("/flag")]
            "#,
        )
        .unwrap();
    assert_eq!(values[0].clone_cast::<ImmutableString>().as_str(), "007");
    assert_eq!(values[1].as_int().unwrap(), 7);
    assert!(values[2].as_bool().unwrap());
}

#[test]
fn pointer_and_schema_failures_are_stable_and_do_not_mutate_the_document() {
    for (script, code) in [
        (
            r#"let d = document::create(); d.get("text")"#,
            ErrorCode::DocumentPointerInvalid,
        ),
        (
            r#"let d = document::create(); d.get("/missing")"#,
            ErrorCode::DocumentPathMissing,
        ),
        (
            r#"let d = document::create(); d.set("/number", "7")"#,
            ErrorCode::DocumentFieldTypeMismatch,
        ),
    ] {
        let error = engine().eval::<Dynamic>(script).unwrap_err().to_string();
        assert!(error.contains(code.as_str()), "{error}");
    }

    let mut scope = rhai::Scope::new();
    scope.push(
        "document",
        Document::new(DocumentValue::Object(BTreeMap::default())),
    );
    let _ = engine()
        .eval_with_scope::<Dynamic>(&mut scope, r#"document.set("/number", 7)"#)
        .unwrap();
    let error = engine()
        .eval_with_scope::<Dynamic>(&mut scope, r#"document.set("/number", "wrong")"#)
        .unwrap_err()
        .to_string();
    assert!(error.contains(ErrorCode::DocumentFieldTypeMismatch.as_str()));
    let document = scope.get_value::<Document>("document").unwrap();
    assert_eq!(
        document
            .resolve(&JsonPointer::parse("/number").unwrap())
            .unwrap(),
        &DocumentValue::integer(7).unwrap()
    );
}

#[test]
fn fractional_document_numbers_fail_in_the_integer_only_rhai_host() {
    let mut scope = rhai::Scope::new();
    scope.push(
        "document",
        Document::parse_json(r#"{"number":1.5}"#).unwrap(),
    );
    let error = engine()
        .eval_with_scope::<Dynamic>(&mut scope, r#"document.get("/number")"#)
        .unwrap_err()
        .to_string();
    assert!(error.contains(ErrorCode::DocumentNumberInvalid.as_str()));
}

#[test]
fn fields_exposes_root_object_entries_without_flat_schema_slots() {
    let rows = engine()
        .eval::<Array>(r#"let d = document::create(); d.set("/number", 42); d.fields()"#)
        .unwrap();
    assert_eq!(rows.len(), 1);
    let row = rows[0].clone_cast::<Map>();
    assert_eq!(
        row["name"].clone_cast::<ImmutableString>().as_str(),
        "number"
    );
    assert_eq!(row["value"].as_int().unwrap(), 42);
}
