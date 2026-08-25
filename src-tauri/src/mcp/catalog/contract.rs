//! Shared machine-readable contract helpers for the static MCP catalog.

use std::sync::Arc;

use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
enum OutputRoot {
    Object,
    Array,
    ObjectOrNull,
}

pub(super) fn output_schema(name: &str) -> Arc<Map<String, Value>> {
    let schema = match output_root(name)
        .unwrap_or_else(|| panic!("MCP tool {name} is missing an explicit output root contract"))
    {
        OutputRoot::Array => json!({
            "type": "array",
            "description": "Successful structured result. See the MCP tool reference resource for the retained fields and paging contract.",
            "items": {}
        }),
        OutputRoot::ObjectOrNull => json!({
            "type": ["object", "null"],
            "description": "Successful structured result. Null means no Android profile currently owns the runtime.",
            "additionalProperties": true
        }),
        OutputRoot::Object => json!({
            "type": "object",
            "description": "Successful structured result. See the MCP tool reference resource for the returned projection and retention contract.",
            "additionalProperties": true
        }),
    };
    let Value::Object(schema) = schema else {
        unreachable!("static output schema is always an object");
    };
    Arc::new(schema)
}

pub(super) fn validate_successful_output(name: &str, value: &Value) -> Result<(), String> {
    let expected = output_root(name)
        .ok_or_else(|| format!("MCP tool {name} is missing an output root contract"))?;
    let matches = match expected {
        OutputRoot::Object => value.is_object(),
        OutputRoot::Array => value.is_array(),
        OutputRoot::ObjectOrNull => value.is_object() || value.is_null(),
    };
    if matches {
        return Ok(());
    }
    let expected = match expected {
        OutputRoot::Object => "object",
        OutputRoot::Array => "array",
        OutputRoot::ObjectOrNull => "object or null",
    };
    Err(format!(
        "{name} returned {}, but its successful output schema requires {expected}",
        value_kind(value)
    ))
}

pub(super) fn describe_properties(properties: &mut Map<String, Value>) {
    for (name, schema) in properties {
        let Value::Object(schema) = schema else {
            continue;
        };
        schema
            .entry("description")
            .or_insert_with(|| Value::String(property_description(name).to_owned()));
        if let Some(Value::Object(nested)) = schema.get_mut("properties") {
            describe_properties(nested);
        }
    }
}

pub(super) fn validate_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| "tool arguments must be a JSON object".to_owned())?;
    let tool = super::tools()
        .into_iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| format!("unknown read-only tool: {name}"))?;
    let mut violations = Vec::new();
    validate_object(arguments, tool.input_schema.as_ref(), "", &mut violations);
    violations.sort();
    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "arguments for {name} violate the published input schema: {}",
            violations.join("; ")
        ))
    }
}

fn validate_object(
    value: &Map<String, Value>,
    schema: &Map<String, Value>,
    path: &str,
    violations: &mut Vec<String>,
) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for required in required.iter().filter_map(Value::as_str) {
            if !value.contains_key(required) {
                violations.push(field_path(path, required));
            }
        }
    }
    for (name, child) in value {
        let child_path = field_path(path, name);
        let Some(child_schema) = properties.get(name).and_then(Value::as_object) else {
            if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                violations.push(child_path);
            }
            continue;
        };
        validate_value(child, child_schema, &child_path, violations);
    }
}

fn validate_value(
    value: &Value,
    schema: &Map<String, Value>,
    path: &str,
    violations: &mut Vec<String>,
) {
    if !schema_type_matches(value, schema.get("type")) {
        violations.push(format!(
            "{path} must be {}, got {}",
            schema_type_label(schema.get("type")),
            value_kind(value)
        ));
        return;
    }
    if value.is_null() {
        return;
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array)
        && !allowed.contains(value)
    {
        violations.push(format!("{path} is not an allowed value"));
    }
    if let Some(number) = value.as_f64() {
        if let Some(minimum) = schema.get("minimum").and_then(Value::as_f64)
            && number < minimum
        {
            violations.push(format!("{path} is below minimum {minimum}"));
        }
        if let Some(maximum) = schema.get("maximum").and_then(Value::as_f64)
            && number > maximum
        {
            violations.push(format!("{path} exceeds maximum {maximum}"));
        }
    }
    if let Some(text) = value.as_str() {
        match schema.get("format").and_then(Value::as_str) {
            Some("uuid") if !is_uuid(text) => {
                violations.push(format!("{path} must be a UUID"));
            }
            Some("date-time") if chrono::DateTime::parse_from_rfc3339(text).is_err() => {
                violations.push(format!("{path} must be an RFC 3339 date-time"));
            }
            _ => {}
        }
    }
    if let Some(object) = value.as_object() {
        validate_object(object, schema, path, violations);
    }
}

fn schema_type_matches(value: &Value, schema_type: Option<&Value>) -> bool {
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "integer" => value
            .as_number()
            .is_some_and(|number| number.is_i64() || number.is_u64()),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    match schema_type {
        None => true,
        Some(Value::String(kind)) => matches(kind),
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).any(matches),
        Some(_) => false,
    }
}

fn schema_type_label(schema_type: Option<&Value>) -> String {
    match schema_type {
        Some(Value::String(kind)) => kind.clone(),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" or "),
        _ => "the documented type".to_owned(),
    }
}

fn field_path(path: &str, field: &str) -> String {
    if path.is_empty() {
        field.to_owned()
    } else {
        format!("{path}.{field}")
    }
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn output_root(name: &str) -> Option<OutputRoot> {
    match name {
        "workspace_list"
        | "entry_status_list"
        | "android_device_list"
        | "android_package_list"
        | "android_profile_list"
        | "workspace_certificate_overview"
        | "breakpoint_query"
        | "http_rule_list"
        | "protocol_rule_list"
        | "workspace_protocol_rule_list"
        | "protocol_package_list"
        | "protocol_package_usage" => Some(OutputRoot::Array),
        "android_runtime_owner" => Some(OutputRoot::ObjectOrNull),
        "application_snapshot"
        | "application_log_query"
        | "application_log_get"
        | "exchange_observation_query"
        | "exchange_observation_get"
        | "reproduction_report"
        | "settings_get"
        | "workspace_get"
        | "entry_overview"
        | "diagnostics_query"
        | "diagnose_recent_failures"
        | "external_package_service_status"
        | "android_adb_get"
        | "android_package_get"
        | "android_profile_get"
        | "android_network_status"
        | "android_network_endpoints"
        | "certificate_overview"
        | "http_capture_query"
        | "http_capture_get"
        | "breakpoint_get"
        | "http_rule_get"
        | "protocol_package_catalog"
        | "protocol_package_detail" => Some(OutputRoot::Object),
        _ => None,
    }
}

fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn property_description(name: &str) -> &'static str {
    match name {
        "workspace_id" => "Workspace UUID used to scope the read.",
        "listener_id" => "Listener or entry UUID used to scope the read.",
        "log_id" => "Stable retained application-log ID.",
        "exchange_id" => "Stable Exchange tracing correlation ID.",
        "page" => "One-based page number or nested paging object.",
        "page_size" => "Maximum number of retained rows returned per page.",
        "package" => "Exact immutable protocol package identity.",
        "id" => "Protocol package ID.",
        "version" => "Exact protocol package version.",
        "keyword" => "Case-insensitive text filter applied to searchable retained fields.",
        "after_event_id" => "Return retained events after this stable event ID.",
        "limit" => "Maximum number of retained records to return.",
        "level" => "Minimum or exact runtime-log level filter.",
        "target" => "Rust tracing target or module filter.",
        "occurred_from" => "Inclusive RFC 3339 lower timestamp bound.",
        "occurred_to" => "Inclusive RFC 3339 upper timestamp bound.",
        "before_log_id" => "Return older logs before this stable log ID.",
        "profile_id" => "Android network profile ID; omit where the active profile is allowed.",
        "package_name" => "Android application package name.",
        "session_id" => "HTTP capture session UUID.",
        "runtime_epoch" => "Runtime epoch UUID that prevents stale capture or breakpoint reads.",
        "breakpoint_id" => "Pending HTTP breakpoint UUID.",
        "rule_id" => "HTTP rule UUID or optional capture rule filter.",
        "terminal_ip" => "Terminal IP address filter.",
        "channel" => "Capture channel filter.",
        "stage" => "Capture lifecycle-stage filter.",
        "result" => "Capture result filter.",
        "sort" => "Capture sort field.",
        "direction" => "Ascending or descending sort direction.",
        _ => "Tool-specific read-only input field.",
    }
}
