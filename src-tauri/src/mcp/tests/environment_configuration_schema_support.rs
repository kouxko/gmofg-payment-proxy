use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::super::super::environment_contract::environment_contract_tools;

pub(super) fn schema_snapshot() -> Value {
    serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/schema.snapshot.json"
    ))
    .expect("schema snapshot is valid JSON")
}

pub(super) fn expected_preview() -> Value {
    serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/expected-preview.json"
    ))
    .expect("expected preview is valid JSON")
}

pub(super) fn literal_coverage() -> Value {
    serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/literal-coverage.json"
    ))
    .expect("literal coverage fixture is valid JSON")
}

pub(super) fn published_schemas() -> Value {
    let tools = environment_contract_tools()
        .into_iter()
        .map(|tool| {
            let output = tool.output_schema.expect("environment tool output schema");
            (
                tool.name.into_owned(),
                json!({
                    "inputSchema": Value::Object((*tool.input_schema).clone()),
                    "outputSchema": Value::Object((*output).clone())
                }),
            )
        })
        .collect::<Map<_, _>>();
    Value::Object(tools)
}

pub(super) fn string_set(value: &Value) -> BTreeSet<&str> {
    value
        .as_array()
        .expect("literal category is an array")
        .iter()
        .map(|item| item.as_str().expect("literal is a string"))
        .collect()
}

pub(super) fn literal_array<'a>(
    coverage: &'a Value,
    category: &str,
) -> impl Iterator<Item = &'a str> {
    coverage[category]
        .as_array()
        .expect("literal category is an array")
        .iter()
        .map(|value| value.as_str().expect("literal is a string"))
}

pub(super) fn contains_key(value: &Value, forbidden: &str) -> bool {
    match value {
        Value::Object(object) => {
            object.contains_key(forbidden) || object.values().any(|v| contains_key(v, forbidden))
        }
        Value::Array(values) => values.iter().any(|v| contains_key(v, forbidden)),
        _ => false,
    }
}

pub(super) fn extract_contract_literals(
    value: &Value,
    parent: Option<&str>,
    output: &mut BTreeSet<String>,
) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for (field, schema) in properties {
                    extract_contract_literals(schema, Some(field), output);
                }
            }
            for (key, child) in object {
                if matches!(key.as_str(), "const" | "enum") && is_registered_literal_field(parent) {
                    collect_string_literals(child, output);
                } else if key != "properties" {
                    extract_contract_literals(child, parent, output);
                }
            }
        }
        Value::Array(values) => {
            for child in values {
                extract_contract_literals(child, parent, output);
            }
        }
        _ => {}
    }
}

fn is_registered_literal_field(field: Option<&str>) -> bool {
    matches!(
        field,
        Some(
            "protocol_version"
                | "authentication"
                | "source_ip_filter"
                | "host_header_policy"
                | "origin_policy"
                | "authorization_policy"
                | "warnings"
                | "eviction"
                | "evicted_status_code"
                | "schema_versions"
                | "validation_layers"
                | "expires_on"
                | "status"
                | "result"
                | "status_code"
                | "code"
                | "severity"
                | "layer"
        )
    )
}

fn collect_string_literals(value: &Value, output: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            output.insert(text.clone());
        }
        Value::Array(values) => {
            for child in values {
                collect_string_literals(child, output);
            }
        }
        _ => {}
    }
}

pub(super) fn extract_positive_contract_literals<'a>(
    value: &'a Value,
    field: Option<&str>,
    output: &mut BTreeSet<&'a str>,
) {
    if is_registered_literal_field(field) {
        match value {
            Value::String(text) => {
                output.insert(text);
            }
            Value::Array(values) => {
                for child in values {
                    extract_positive_contract_literals(child, field, output);
                }
            }
            _ => {}
        }
    }
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                extract_positive_contract_literals(child, Some(key), output);
            }
        }
        Value::Array(values) => {
            for child in values {
                extract_positive_contract_literals(child, field, output);
            }
        }
        _ => {}
    }
}
