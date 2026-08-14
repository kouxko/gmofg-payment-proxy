use std::{collections::BTreeMap, fmt::Write};

use intercept_proxy_domain::{
    DocumentField, DocumentFieldName, DocumentFieldType, DocumentSchema, DocumentSchemaId,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use crate::{
    CompiledProtocolPackage, PackageFilePath, ProtocolPackageCompiler, ProtocolPackageFiles,
};

pub(crate) struct CompiledProtocolPackageTestBuilder {
    package: ProtocolPackageRef,
    schema: DocumentSchema,
}

impl CompiledProtocolPackageTestBuilder {
    pub(crate) fn new() -> Self {
        Self {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("test-protocol").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            schema: DocumentSchema::new(
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
        }
    }

    pub(crate) fn with_package(mut self, package: ProtocolPackageRef) -> Self {
        self.package = package;
        self
    }

    pub(crate) fn with_schema(mut self, schema: DocumentSchema) -> Self {
        self.schema = schema;
        self
    }

    pub(crate) fn build(self) -> CompiledProtocolPackage {
        let manifest = format!(
            r#"api = 1

[package]
id = "{}"
name = "Test Protocol"
version = "{}"

[document]
schema = "document.toml"

[hooks.upstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"

[hooks.downstream.receive]
script = "protocol.rhai"
frame = "frame"
decode = "decode"
"#,
            self.package.id, self.package.version
        );
        let schema = schema_toml(&self.schema);
        let script = b"fn frame(reader, context) { () }\nfn decode(origin, context) { () }\n";
        let total_bytes = manifest.len() + schema.len() + script.len();
        let files = ProtocolPackageFiles::new(
            BTreeMap::from([
                (
                    PackageFilePath::new("manifest.toml").unwrap(),
                    manifest.into_bytes(),
                ),
                (
                    PackageFilePath::new("document.toml").unwrap(),
                    schema.into_bytes(),
                ),
                (
                    PackageFilePath::new("protocol.rhai").unwrap(),
                    script.to_vec(),
                ),
            ]),
            u64::try_from(total_bytes).unwrap(),
        );
        ProtocolPackageCompiler::default()
            .compile(&files)
            .expect("test builder only emits a valid compiled package")
    }
}

fn schema_toml(schema: &DocumentSchema) -> String {
    let mut output = format!(
        "id = {}\nversion = {}\ntitle = {}\n",
        serde_json::to_string(schema.id().as_str()).unwrap(),
        schema.version(),
        serde_json::to_string(schema.title()).unwrap(),
    );
    for field in schema.fields() {
        write!(
            output,
            "\n[[fields]]\nname = {}\nlabel = {}\ntype = {}\n",
            serde_json::to_string(field.name().as_str()).unwrap(),
            serde_json::to_string(field.label()).unwrap(),
            serde_json::to_string(field.field_type().as_str()).unwrap(),
        )
        .unwrap();
    }
    output
}
