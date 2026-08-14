use std::sync::Arc;

use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use crate::CompiledProtocolPackage;

pub(crate) struct CompiledProtocolPackageTestBuilder {
    package: ProtocolPackageRef,
    schema: Arc<DocumentSchema>,
}

impl CompiledProtocolPackageTestBuilder {
    pub(crate) fn new() -> Self {
        Self {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("test-protocol").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            schema: Arc::new(
                DocumentSchema::new(
                    DocumentSchemaId::new("test-message").unwrap(),
                    1,
                    "Test message",
                    vec![
                        DocumentField::new(
                            DocumentFieldName::new("amount").unwrap(),
                            DocumentFieldType::Int,
                            "Amount",
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            ),
        }
    }

    pub(crate) fn with_package(mut self, package: ProtocolPackageRef) -> Self {
        self.package = package;
        self
    }

    pub(crate) fn with_schema(mut self, schema: DocumentSchema) -> Self {
        self.schema = Arc::new(schema);
        self
    }

    pub(crate) fn build(self) -> CompiledProtocolPackage {
        CompiledProtocolPackage::new(self.package, self.schema)
    }
}
