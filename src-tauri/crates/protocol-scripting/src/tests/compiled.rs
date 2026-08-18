use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use crate::ProtocolDirection;
use crate::test_support::CompiledProtocolPackageTestBuilder;

#[test]
fn compiled_package_test_builder_preserves_exact_identity_and_shared_schema() {
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("iso8583-standard").unwrap(),
        version: ProtocolPackageVersion::new("2.1.0-beta.1").unwrap(),
    };
    let schema = DocumentSchema::new(
        DocumentSchemaId::new("iso8583-message").unwrap(),
        2,
        "ISO 8583:1987 Message",
        vec![
            DocumentField::new(
                DocumentFieldName::new("mti").unwrap(),
                DocumentFieldType::String,
                "Message type",
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let compiled = CompiledProtocolPackageTestBuilder::new()
        .with_package(package.clone())
        .with_schema(schema.clone())
        .build();

    assert_eq!(compiled.package(), &package);
    assert_eq!(compiled.manifest().package().package(), &package);
    assert_eq!(compiled.schema(ProtocolDirection::Upstream), &schema);
    assert!(compiled.supports_upstream_encode());
    assert!(compiled.supports_downstream_encode());

    let cloned = compiled.clone();
    assert_eq!(cloned.package(), &package);
    assert!(std::ptr::eq(
        compiled.schema(ProtocolDirection::Upstream),
        cloned.schema(ProtocolDirection::Upstream)
    ));
}

#[test]
fn compiled_package_test_builder_has_valid_safe_defaults() {
    let compiled = CompiledProtocolPackageTestBuilder::new().build();

    assert_eq!(compiled.package().id.as_str(), "test-protocol");
    assert_eq!(compiled.package().version.as_str(), "1.0.0");
    assert_eq!(
        compiled.schema(ProtocolDirection::Upstream).id().as_str(),
        "test-message"
    );
    assert_eq!(
        compiled.schema(ProtocolDirection::Upstream).fields().len(),
        1
    );
}
