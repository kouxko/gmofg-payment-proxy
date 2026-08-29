use std::{collections::BTreeMap, fmt::Write};

use intercept_proxy_domain::{
    DocumentSchemaNode, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};

use crate::{
    CompiledProtocolPackage, PackageFilePath, ProtocolPackageCompiler, ProtocolPackageFiles,
};

pub(crate) struct CompiledProtocolPackageTestBuilder {
    package: ProtocolPackageRef,
    upstream_schema: DocumentSchemaNode,
    downstream_schema: DocumentSchemaNode,
    script: String,
}

impl CompiledProtocolPackageTestBuilder {
    pub(crate) fn new() -> Self {
        Self {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("test-protocol").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            upstream_schema: amount_schema("Test message"),
            downstream_schema: amount_schema("Test message"),
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

    pub(crate) fn with_schema(mut self, schema: DocumentSchemaNode) -> Self {
        self.upstream_schema = schema.clone();
        self.downstream_schema = schema;
        self
    }

    pub(crate) fn with_directional_schemas(
        mut self,
        upstream: DocumentSchemaNode,
        downstream: DocumentSchemaNode,
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

fn amount_schema(title: &str) -> DocumentSchemaNode {
    DocumentSchemaNode::Object {
        title: Some(title.to_owned()),
        properties: BTreeMap::from([(
            "amount".to_owned(),
            DocumentSchemaNode::Number {
                title: Some("Amount".to_owned()),
            },
        )]),
    }
}

fn schema_toml(schema: &DocumentSchemaNode) -> String {
    let mut output = String::new();
    write_schema_node(&mut output, schema, None);
    output
}

fn write_schema_node(output: &mut String, schema: &DocumentSchemaNode, table: Option<&str>) {
    if let Some(table) = table {
        writeln!(output, "\n[{table}]").unwrap();
    }
    let (kind, title) = match schema {
        DocumentSchemaNode::String { title } => ("string", title),
        DocumentSchemaNode::Number { title } => ("number", title),
        DocumentSchemaNode::Boolean { title } => ("boolean", title),
        DocumentSchemaNode::Object { title, .. } => ("object", title),
        DocumentSchemaNode::Array { title, .. } => ("array", title),
    };
    writeln!(output, "type = {kind:?}").unwrap();
    if let Some(title) = title {
        writeln!(output, "title = {}", serde_json::to_string(title).unwrap()).unwrap();
    }
    match schema {
        DocumentSchemaNode::Object { properties, .. } => {
            let parent = table.map_or_else(
                || "properties".to_owned(),
                |table| format!("{table}.properties"),
            );
            for (name, child) in properties {
                write_schema_node(output, child, Some(&format!("{parent}.{}", toml_key(name))));
            }
        }
        DocumentSchemaNode::Array { items, .. } => {
            let table = table.map_or_else(|| "items".to_owned(), |table| format!("{table}.items"));
            write_schema_node(output, items, Some(&table));
        }
        _ => {}
    }
}

fn toml_key(value: &str) -> String {
    serde_json::to_string(value).expect("JSON and TOML basic-string escaping overlap for test keys")
}
