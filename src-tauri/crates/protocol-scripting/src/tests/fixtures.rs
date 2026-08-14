pub(super) const TEMPLATE_MANIFEST: &str =
    include_str!("../../../../../templates/socket-protocol/iso8583-standard/manifest.toml");
pub(super) const TEMPLATE_SCHEMA: &str =
    include_str!("../../../../../templates/socket-protocol/iso8583-standard/document.toml");

pub(super) fn minimal_manifest() -> String {
    r#"
api = 1

[package]
id = "example-protocol"
name = "Example Protocol"
version = "1.2.3-beta.1+build.7"

[document]
schema = "schemas/document.toml"

[hooks.upstream.receive]
script = "scripts/upstream.rhai"
frame = "upstream_frame"
decode = "upstream_decode"

[hooks.downstream.receive]
script = "scripts/downstream.rhai"
frame = "downstream_frame"
decode = "downstream_decode"
"#
    .to_owned()
}

pub(super) fn schema_with_fields(fields: &str) -> String {
    format!(
        r#"
id = "example-message"
version = 1
title = "Example Message"
{fields}
"#
    )
}

pub(super) fn one_field_schema() -> String {
    schema_with_fields(
        r#"
[[fields]]
name = "amount"
label = "Amount"
type = "int"
"#,
    )
}
