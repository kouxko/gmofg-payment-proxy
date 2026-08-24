//! Shared machine-readable contract helpers for the static MCP catalog.

use std::sync::Arc;

use serde_json::{Map, Value, json};

const ARRAY_RESULTS: &[&str] = &[
    "workspace_list",
    "entry_status_list",
    "android_device_list",
    "android_package_list",
    "android_profile_list",
    "workspace_certificate_overview",
    "breakpoint_query",
    "http_rule_list",
    "protocol_rule_list",
    "workspace_protocol_rule_list",
    "protocol_package_list",
    "protocol_package_usage",
];

pub(super) fn output_schema(name: &str) -> Arc<Map<String, Value>> {
    let schema = if ARRAY_RESULTS.contains(&name) {
        json!({
            "type": "array",
            "description": "Successful structured result. See the MCP tool reference resource for the retained fields and paging contract.",
            "items": {}
        })
    } else if name == "android_runtime_owner" {
        json!({
            "type": ["object", "null"],
            "description": "Successful structured result. Null means no Android profile currently owns the runtime.",
            "additionalProperties": true
        })
    } else {
        json!({
            "type": "object",
            "description": "Successful structured result. See the MCP tool reference resource for the returned projection and retention contract.",
            "additionalProperties": true
        })
    };
    let Value::Object(schema) = schema else {
        unreachable!("static output schema is always an object");
    };
    Arc::new(schema)
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

pub(super) fn validate_top_level_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    let arguments = arguments
        .as_object()
        .ok_or_else(|| "tool arguments must be a JSON object".to_owned())?;
    let tool = super::tools()
        .into_iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| format!("unknown read-only tool: {name}"))?;
    let allowed = tool
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("static MCP input schema properties must be an object");
    let mut unknown = arguments
        .keys()
        .filter(|key| !allowed.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    unknown.sort();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unknown top-level argument(s) for {name}: {}",
            unknown.join(", ")
        ))
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
