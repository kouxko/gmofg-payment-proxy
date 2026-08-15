use std::sync::Arc;

use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
};
use rhai::{Dynamic, EvalAltResult, Position};

use crate::ProtocolResourceLimit;

use super::{find_resource_limit, validate_document_schema};

#[test]
fn document_schema_identity_is_checked_at_runtime_boundary() {
    let expected = DocumentSchema::new(
        DocumentSchemaId::new("expected").unwrap(),
        1,
        "Expected",
        vec![
            DocumentField::new(
                DocumentFieldName::new("amount").unwrap(),
                DocumentFieldType::Int,
                "Amount",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let other = DocumentSchema::new(
        DocumentSchemaId::new("other").unwrap(),
        1,
        "Other",
        vec![
            DocumentField::new(
                DocumentFieldName::new("trace").unwrap(),
                DocumentFieldType::String,
                "Trace",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let document = intercept_proxy_domain::Document::new(Arc::new(other));

    assert!(validate_document_schema(&document, &expected).is_err());
}

#[test]
fn nested_rhai_resource_errors_map_without_using_display_text() {
    let string = EvalAltResult::ErrorDataTooLarge("Length of string".to_owned(), Position::NONE);
    let nested = EvalAltResult::ErrorInFunctionCall(
        "helper".to_owned(),
        "safe-source".to_owned(),
        Box::new(string),
        Position::NONE,
    );
    assert_eq!(
        find_resource_limit(&nested),
        Some(ProtocolResourceLimit::StringBytes)
    );

    let nested_module = EvalAltResult::ErrorInModule(
        "module".to_owned(),
        Box::new(EvalAltResult::ErrorTerminated(
            Dynamic::UNIT,
            Position::NONE,
        )),
        Position::NONE,
    );
    assert_eq!(
        find_resource_limit(&nested_module),
        Some(ProtocolResourceLimit::WallTimeMs)
    );
    assert_eq!(
        find_resource_limit(&EvalAltResult::ErrorRuntime(Dynamic::UNIT, Position::NONE)),
        None
    );
}
