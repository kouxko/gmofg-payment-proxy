mod document;
mod schema;

use super::*;

fn field(name: &str, field_type: DocumentFieldType) -> DocumentField {
    DocumentField::new(
        DocumentFieldName::new(name).unwrap(),
        field_type,
        name.to_uppercase(),
    )
    .unwrap()
}

fn four_field_schema() -> DocumentSchema {
    DocumentSchema::new(
        DocumentSchemaId::new("payment-message").unwrap(),
        1,
        "Payment Message",
        vec![
            field("merchant", DocumentFieldType::String),
            field("amount", DocumentFieldType::Int),
            field("approved", DocumentFieldType::Bool),
            field("raw", DocumentFieldType::Blob),
        ],
    )
    .unwrap()
}
