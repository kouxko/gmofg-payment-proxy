pub(super) const TEMPLATE_MANIFEST: &str =
    include_str!("../../../../../templates/socket-protocol/iso8583-standard/manifest.toml");
pub(super) const TEMPLATE_SCHEMA: &str =
    include_str!("../../../../../templates/socket-protocol/iso8583-standard/document.toml");
pub(super) const TEMPLATE_PROTOCOL: &[u8] =
    include_bytes!("../../../../../templates/socket-protocol/iso8583-standard/protocol.rhai");
pub(super) const TEMPLATE_DISPLAY: &[u8] =
    include_bytes!("../../../../../templates/socket-protocol/iso8583-standard/display.rhai");
pub(super) const TEMPLATE_LIBRARY: &[u8] = include_bytes!(
    "../../../../../templates/socket-protocol/iso8583-standard/libraries/iso8583.rhai"
);
pub(super) const TEMPLATE_REQUEST_SAMPLE: &str = include_str!(
    "../../../../../templates/socket-protocol/iso8583-standard/samples/financial-request.json"
);
pub(super) const TEMPLATE_RESPONSE_SAMPLE: &str = include_str!(
    "../../../../../templates/socket-protocol/iso8583-standard/samples/financial-response.json"
);

pub(super) fn minimal_manifest() -> String {
    r#"
api = 1

[package]
id = "example-protocol"
name = "Example Protocol"
version = "1.2.3-beta.1+build.7"

[document.upstream]
schema = "schemas/upstream.toml"
display = "render_upstream"

[document.downstream]
schema = "schemas/downstream.toml"
display = "render_downstream"

[hooks.upstream]
frame = "upstream_frame"
decode = "upstream_decode"
encode = "upstream_encode"

[hooks.downstream]
frame = "downstream_frame"
decode = "downstream_decode"
encode = "downstream_encode"
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
