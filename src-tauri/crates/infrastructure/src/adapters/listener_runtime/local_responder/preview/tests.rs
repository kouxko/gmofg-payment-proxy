use intercept_proxy_domain::{
    Document, DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema,
    DocumentSchemaId, DocumentValue,
};

use super::*;

#[test]
fn preview_value_covers_four_types_and_zero_budget_omission() {
    assert_eq!(
        preview_value(&DocumentValue::String("éx".into()), 2),
        (Some("é".into()), true, false)
    );
    assert_eq!(
        preview_value(&DocumentValue::String("x".into()), 0),
        (None, false, true)
    );
    assert_eq!(
        preview_value(&DocumentValue::Int(12), 2),
        (Some("12".into()), false, false)
    );
    assert_eq!(
        preview_value(&DocumentValue::Int(12), 1),
        (None, false, true)
    );
    assert_eq!(
        preview_value(&DocumentValue::Bool(true), 4),
        (Some("true".into()), false, false)
    );
    assert_eq!(
        preview_value(&DocumentValue::Blob(vec![0xab, 0xcd]), 2),
        (Some("ab".into()), true, false)
    );
    assert_eq!(
        preview_value(&DocumentValue::Blob(vec![1]), 0),
        (None, false, true)
    );
}

#[test]
fn document_preview_preserves_sparse_order_and_marks_budget_truncation() {
    let mut document = document_with_label("small");
    document
        .set("text", DocumentValue::String("value".into()))
        .unwrap();
    document.set("count", DocumentValue::Int(7)).unwrap();
    document.set("flag", DocumentValue::Bool(true)).unwrap();
    document
        .set("raw", DocumentValue::Blob(vec![0xaa, 0xbb]))
        .unwrap();
    let preview = document_preview(&document);
    assert_eq!(preview.fields.len(), 5);
    assert!(!preview.truncated);
    assert!(!preview.fields[4].present);

    let huge = document_many_fields(128, &"x".repeat(128));
    let truncated = document_preview(&huge);
    assert!(truncated.truncated);
    assert!(truncated.fields.len() < 128);
}

#[test]
fn utf8_truncation_never_splits_a_character() {
    assert_eq!(truncate_utf8("abc", 3), "abc");
    assert_eq!(truncate_utf8("éx", 1), "");
    assert_eq!(truncate_utf8("éx", 2), "é");
}

fn document_with_label(label: &str) -> Document {
    let fields = [
        ("text", DocumentFieldType::String),
        ("count", DocumentFieldType::Int),
        ("flag", DocumentFieldType::Bool),
        ("raw", DocumentFieldType::Blob),
        ("unset", DocumentFieldType::String),
    ]
    .into_iter()
    .map(|(name, ty)| DocumentField::new(DocumentFieldName::new(name).unwrap(), ty, label).unwrap())
    .collect();
    Document::new(
        DocumentSchema::new(
            DocumentSchemaId::new("preview").unwrap(),
            1,
            "Preview",
            fields,
        )
        .unwrap(),
    )
}

fn document_many_fields(count: usize, label: &str) -> Document {
    let fields = (0..count)
        .map(|index| {
            DocumentField::new(
                DocumentFieldName::new(format!("field_{index}")).unwrap(),
                DocumentFieldType::String,
                label,
            )
            .unwrap()
        })
        .collect();
    Document::new(
        DocumentSchema::new(
            DocumentSchemaId::new("preview-many").unwrap(),
            1,
            "Preview many",
            fields,
        )
        .unwrap(),
    )
}
