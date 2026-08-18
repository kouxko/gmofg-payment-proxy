use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
};
use rhai::Engine;

use crate::{
    ProtocolDirection, ProtocolRuntimeLimits, compiler::build_engine, host::ProtocolHostApi,
    test_support::CompiledProtocolPackageTestBuilder,
};

pub(super) fn schema() -> DocumentSchema {
    DocumentSchema::new(
        DocumentSchemaId::new("host-test-message").unwrap(),
        3,
        "Host Test Message",
        vec![
            field("text_value", DocumentFieldType::String, "Text"),
            field("int_value", DocumentFieldType::Int, "Integer"),
            field("bool_value", DocumentFieldType::Bool, "Boolean"),
            field("blob_value", DocumentFieldType::Blob, "Binary"),
        ],
    )
    .unwrap()
}

pub(super) fn host() -> ProtocolHostApi {
    let package = CompiledProtocolPackageTestBuilder::new()
        .with_schema(schema())
        .build();
    ProtocolHostApi::for_package(&package, ProtocolDirection::Upstream)
}

pub(super) fn engine() -> Engine {
    let host = host();
    let mut engine = build_engine(ProtocolRuntimeLimits::default());
    host.register(&mut engine);
    engine
}

fn field(name: &str, field_type: DocumentFieldType, label: &str) -> DocumentField {
    DocumentField::new(DocumentFieldName::new(name).unwrap(), field_type, label).unwrap()
}
