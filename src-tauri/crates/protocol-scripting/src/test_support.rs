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
    upstream_schema: DocumentSchema,
    downstream_schema: DocumentSchema,
    script: String,
}

impl CompiledProtocolPackageTestBuilder {
    pub(crate) fn new() -> Self {
        Self {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("test-protocol").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            upstream_schema: DocumentSchema::new(
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
            downstream_schema: DocumentSchema::new(
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
            script: concat!(
                "fn frame(reader, context) { () }\n",
                "fn decode(origin, context) { document::create() }\n",
                "fn encode(origin, document, context) { origin }\n",
                "fn display(document, context) { \"<p>ok</p>\" }\n",
            )
            .to_owned(),
        }
    }

    pub(crate) fn with_package(mut self, package: ProtocolPackageRef) -> Self {
        self.package = package;
        self
    }

    pub(crate) fn with_schema(mut self, schema: DocumentSchema) -> Self {
        self.upstream_schema = schema.clone();
        self.downstream_schema = schema;
        self
    }

    pub(crate) fn with_directional_schemas(
        mut self,
        upstream: DocumentSchema,
        downstream: DocumentSchema,
    ) -> Self {
        self.upstream_schema = upstream;
        self.downstream_schema = downstream;
        self
    }

    pub(crate) fn with_script(mut self, script: impl Into<String>) -> Self {
        self.script = script.into();
        self
    }

    pub(crate) const fn with_upstream_encode(self) -> Self {
        self
    }

    pub(crate) const fn with_downstream_encode(self) -> Self {
        self
    }

    pub(crate) const fn with_display(self) -> Self {
        self
    }

    pub(crate) fn build(self) -> CompiledProtocolPackage {
        let manifest = format!(
            r#"api = 1

[package]
id = "{}"
name = "Test Protocol"
version = "{}"

[document.upstream]
schema = "upstream.toml"
display = "display"

[document.downstream]
schema = "downstream.toml"
display = "display"

[hooks.upstream]
frame = "frame"
decode = "decode"
encode = "encode"

[hooks.downstream]
frame = "frame"
decode = "decode"
encode = "encode"
"#,
            self.package.id, self.package.version
        );
        let mut script = self.script;
        if !script.contains("fn encode(") {
            script.push_str("\nfn encode(origin, document, context) { origin }\n");
        }
        let display = if script.contains("fn display(") {
            script.clone()
        } else {
            "fn display(document, context) { \"<p>ok</p>\" }\n".to_owned()
        };
        let upstream_schema = schema_toml(&self.upstream_schema);
        let downstream_schema = schema_toml(&self.downstream_schema);
        let script = script.into_bytes();
        let total_bytes = manifest.len()
            + upstream_schema.len()
            + downstream_schema.len()
            + script.len()
            + display.len();
        let files = ProtocolPackageFiles::new(
            BTreeMap::from([
                (
                    PackageFilePath::new("manifest.toml").unwrap(),
                    manifest.into_bytes(),
                ),
                (
                    PackageFilePath::new("upstream.toml").unwrap(),
                    upstream_schema.into_bytes(),
                ),
                (
                    PackageFilePath::new("downstream.toml").unwrap(),
                    downstream_schema.into_bytes(),
                ),
                (PackageFilePath::new("protocol.rhai").unwrap(), script),
                (
                    PackageFilePath::new("display.rhai").unwrap(),
                    display.into_bytes(),
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
