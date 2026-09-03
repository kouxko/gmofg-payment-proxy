#![cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]

use serde_json::Value;

wit_bindgen::generate!({
    path: "../../../src-tauri/crates/package-runtime/wit",
    world: "http-package",
});

const _: &str =
    include_str!("../../../../src-tauri/crates/package-runtime/wit/protocol-package.wit");

struct JsonPretty;

impl Guest for JsonPretty {
    fn upstream_decode(input: String) -> Result<String, PackageError> {
        decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn downstream_decode(input: String) -> Result<String, PackageError> {
        decode(&input).map_err(package_error("BODY_DECODE_FAILED"))
    }

    fn upstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        encode(&original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn downstream_encode(
        original_input: String,
        document_json: String,
    ) -> Result<String, PackageError> {
        encode(&original_input, &document_json).map_err(package_error("BODY_ENCODE_FAILED"))
    }

    fn upstream_display(document_json: String) -> Result<String, PackageError> {
        display(&document_json).map_err(package_error("INTERNAL_ERROR"))
    }

    fn downstream_display(document_json: String) -> Result<String, PackageError> {
        display(&document_json).map_err(package_error("INTERNAL_ERROR"))
    }
}

fn decode(input: &str) -> Result<String, String> {
    let document = parse_json(input, "HTTP body")?;
    serde_json::to_string(&document)
        .map_err(|error| format!("cannot serialize decoded JSON document: {error}"))
}

fn encode(original_input: &str, document_json: &str) -> Result<String, String> {
    let document = parse_json(document_json, "Document")?;
    let original = parse_json(original_input, "original HTTP body")?;
    if json_equivalent(&original, &document) {
        return Ok(original_input.to_owned());
    }
    serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot serialize JSON document: {error}"))
}

fn display(document_json: &str) -> Result<String, String> {
    let document = parse_json(document_json, "Document")?;
    let mut html = String::from(
        "<pre style=\"background-color:#1e1e1e;color:#d4d4d4;padding:12px;border-radius:8px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;line-height:1.5;white-space:pre-wrap;overflow-wrap:anywhere\">",
    );
    render_json(&document, 0, &mut html)?;
    html.push_str("</pre>");
    Ok(html)
}

fn render_json(value: &Value, depth: usize, output: &mut String) -> Result<(), String> {
    match value {
        Value::Object(object) => {
            output.push('{');
            if !object.is_empty() {
                output.push('\n');
                for (index, (key, value)) in object.iter().enumerate() {
                    indent(output, depth + 1);
                    output.push_str("<span style=\"color:#9cdcfe\">");
                    output.push_str(&escape_html(&json_string(key)?));
                    output.push_str("</span>: ");
                    render_json(value, depth + 1, output)?;
                    if index + 1 < object.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                indent(output, depth);
            }
            output.push('}');
        }
        Value::Array(array) => {
            output.push('[');
            if !array.is_empty() {
                output.push('\n');
                for (index, value) in array.iter().enumerate() {
                    indent(output, depth + 1);
                    render_json(value, depth + 1, output)?;
                    if index + 1 < array.len() {
                        output.push(',');
                    }
                    output.push('\n');
                }
                indent(output, depth);
            }
            output.push(']');
        }
        Value::String(value) => styled_value("#ce9178", &json_string(value)?, output),
        Value::Number(value) => styled_value("#b5cea8", &value.to_string(), output),
        Value::Bool(value) => {
            styled_value("#569cd6", if *value { "true" } else { "false" }, output)
        }
        Value::Null => styled_value("#569cd6", "null", output),
    }
    Ok(())
}

fn json_string(value: &str) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| format!("cannot serialize JSON display: {error}"))
}

fn styled_value(color: &str, value: &str, output: &mut String) {
    output.push_str("<span style=\"color:");
    output.push_str(color);
    output.push_str("\">");
    output.push_str(&escape_html(value));
    output.push_str("</span>");
}

fn indent(output: &mut String, depth: usize) {
    for _ in 0..depth {
        output.push_str("  ");
    }
}

fn parse_json(input: &str, label: &str) -> Result<Value, String> {
    serde_json::from_str(input).map_err(|error| format!("{label} is not valid JSON: {error}"))
}

fn json_equivalent(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.as_f64() == right.as_f64(),
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| json_equivalent(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, left)| {
                    right
                        .get(key)
                        .is_some_and(|right| json_equivalent(left, right))
                })
        }
        _ => left == right,
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

fn package_error(code: &'static str) -> impl FnOnce(String) -> PackageError {
    move |message| PackageError {
        code: code.to_owned(),
        message,
    }
}

export!(JsonPretty);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_accepts_every_json_root_kind_and_rejects_invalid_input() {
        for input in [
            r#"{"object":{"nested":true}}"#,
            r#"[1,"two",null]"#,
            r#""text""#,
            "42",
            "false",
            "null",
        ] {
            assert_eq!(
                serde_json::from_str::<Value>(&decode(input).expect("decode valid JSON")).unwrap(),
                serde_json::from_str::<Value>(input).unwrap()
            );
        }
        assert!(
            decode(r#"{"missing":}"#)
                .unwrap_err()
                .contains("not valid JSON")
        );
    }

    #[test]
    fn display_pretty_prints_colors_types_and_escapes_html() {
        let html =
            display(r#"{"markup":"<tag attr='x'>&\"","amount":12.5,"enabled":true,"value":null}"#)
                .expect("display JSON");
        assert!(html.starts_with("<pre style=\"background-color:#1e1e1e;"));
        assert!(html.contains("<span style=\"color:#9cdcfe\">&quot;markup&quot;</span>"));
        assert!(html.contains("<span style=\"color:#ce9178\">&quot;&lt;tag attr=&#39;x&#39;&gt;&amp;\\&quot;&quot;</span>"));
        assert!(html.contains("<span style=\"color:#b5cea8\">12.5</span>"));
        assert!(html.contains("<span style=\"color:#569cd6\">true</span>"));
        assert!(html.contains("<span style=\"color:#569cd6\">null</span>"));
        assert!(html.ends_with("\n}</pre>"));
    }

    #[test]
    fn encode_preserves_unchanged_input_and_pretty_prints_changes() {
        let original = "{ \"first\": 1, \"nested\": {\"enabled\": true} }";
        assert_eq!(
            encode(original, r#"{"first":1,"nested":{"enabled":true}}"#).unwrap(),
            original
        );
        assert_eq!(
            encode(original, r#"{"first":2,"nested":{"enabled":true}}"#).unwrap(),
            "{\n  \"first\": 2,\n  \"nested\": {\n    \"enabled\": true\n  }\n}"
        );
        assert_eq!(
            encode(r#"{"amount":100}"#, r#"{"amount":100.0}"#).unwrap(),
            r#"{"amount":100}"#
        );
    }

    #[test]
    fn encode_rejects_invalid_original_and_document_json() {
        assert!(
            encode("not-json", "{}")
                .unwrap_err()
                .contains("original HTTP body")
        );
        assert!(encode("{}", "not-json").unwrap_err().contains("Document"));
    }
}
