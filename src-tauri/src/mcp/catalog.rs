//! Static read-only MCP tool catalog.

mod contract;

use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value, json};

pub(super) fn validate_arguments(name: &str, arguments: &Value) -> Result<(), String> {
    contract::validate_arguments(name, arguments)
}

pub(super) fn validate_successful_output(name: &str, value: &Value) -> Result<(), String> {
    contract::validate_successful_output(name, value)
}

pub fn tools() -> Vec<Tool> {
    let mut tools = Vec::new();
    tools.extend(general_tools());
    tools.extend(runtime_tools());
    tools.extend(traffic_tools());
    tools.extend(configuration_tools());
    tools
}

fn general_tools() -> Vec<Tool> {
    vec![
        tool(
            "application_snapshot",
            "Application snapshot",
            "Read one optimistic, generation-validated snapshot of settings, workspaces, runtime state, packages, rules and diagnostics.",
            empty_schema(),
        ),
        tool(
            "application_log_query",
            "Application runtime logs",
            "Read persisted Rust/Tauri runtime logs with stable cursor paging, explicit retention metadata, module, level and keyword filters.",
            application_log_schema(),
        ),
        tool(
            "application_log_get",
            "Application runtime log detail",
            "Read one retained runtime log by its stable log ID.",
            object_schema(
                json!({"log_id": {"type": "integer", "minimum": 1}}),
                &["log_id"],
            ),
        ),
        tool(
            "exchange_observation_query",
            "Exchange observations",
            "Read connection-level Exchange events from the same bounded memory store used by the UI.",
            object_schema(
                json!({
                    "workspace_id": {"type": "string", "format": "uuid"},
                    "listener_id": {"type": ["string", "null"], "format": "uuid"},
                    "page": {
                        "type": "object",
                        "properties": {
                            "page": {"type": "integer", "minimum": 1},
                            "page_size": {"type": "integer", "minimum": 1, "maximum": 200}
                        },
                        "required": ["page", "page_size"],
                        "additionalProperties": false
                    }
                }),
                &["workspace_id", "page"],
            ),
        ),
        tool(
            "exchange_observation_get",
            "Exchange observation detail",
            "Read one retained connection-level Exchange record by exchange_id.",
            required_string("exchange_id", "Exchange tracing correlation ID."),
        ),
        tool(
            "reproduction_report",
            "Reproduction report",
            "Build one bounded diagnostic bundle and copyable Markdown report for an exact Workspace and Listener. Exchange observations and HTTP captures are queried separately.",
            reproduction_report_schema(),
        ),
        tool(
            "settings_get",
            "Settings",
            "Read the saved global settings.",
            empty_schema(),
        ),
        tool(
            "workspace_list",
            "Workspaces",
            "Read every Workspace summary.",
            empty_schema(),
        ),
        tool(
            "workspace_get",
            "Workspace detail",
            "Read one complete Workspace, including entries, Android profiles, certificate references and protocol rule bindings.",
            required_uuid("workspace_id", "Workspace UUID."),
        ),
        tool(
            "entry_overview",
            "Entry runtime overview",
            "Read configured entries merged with current runtime state for one Workspace.",
            required_uuid("workspace_id", "Workspace UUID."),
        ),
        tool(
            "entry_status_list",
            "Entry statuses",
            "Read current runtime status for every entry.",
            empty_schema(),
        ),
        tool(
            "diagnostics_query",
            "Diagnostics",
            "Read retained structured diagnostics, newest first, with a bounded result count.",
            diagnostics_schema(),
        ),
        tool(
            "diagnose_recent_failures",
            "Troubleshooting suggestions",
            "Read diagnostics and produce deterministic, non-executing UI suggestions with evidence and verification steps.",
            diagnostics_schema(),
        ),
    ]
}

fn runtime_tools() -> Vec<Tool> {
    vec![
        tool(
            "external_package_service_status",
            "External package service",
            "Read the authoritative external-package WebSocket URL, fixed path, bind state, authentication boundary and online connection count.",
            empty_schema(),
        ),
        tool(
            "android_adb_get",
            "Android ADB",
            "Read selected ADB/device state.",
            empty_schema(),
        ),
        tool(
            "android_device_list",
            "Android devices",
            "Read connected Android devices.",
            empty_schema(),
        ),
        tool(
            "android_package_list",
            "Android packages",
            "Read the cached package inventory for the selected device.",
            empty_schema(),
        ),
        tool(
            "android_package_get",
            "Android package detail",
            "Read one Android package from the selected device.",
            required_string("package_name", "Android package name."),
        ),
        tool(
            "android_profile_list",
            "Android network profiles",
            "Read profiles in the selected Workspace.",
            empty_schema(),
        ),
        tool(
            "android_profile_get",
            "Android network profile detail",
            "Read one profile from the selected Workspace.",
            required_string("profile_id", "Android network profile ID."),
        ),
        tool(
            "android_network_status",
            "Android network status",
            "Read current Android network state.",
            empty_schema(),
        ),
        tool(
            "android_runtime_owner",
            "Android runtime owner",
            "Read the persisted profile that owns the active Android runtime.",
            empty_schema(),
        ),
        tool(
            "android_network_endpoints",
            "Android runtime endpoints",
            "Read configured and active runtime endpoints without changing the device.",
            object_schema(json!({"profile_id": {"type": "string"}}), &[]),
        ),
        tool(
            "certificate_overview",
            "Certificate metadata",
            "Read public Root/leaf certificate metadata and readiness; no private key material.",
            empty_schema(),
        ),
        tool(
            "workspace_certificate_overview",
            "Workspace certificate metadata",
            "Read public metadata for managed certificate references in one Workspace; no private key material.",
            required_uuid("workspace_id", "Workspace UUID."),
        ),
    ]
}

fn traffic_tools() -> Vec<Tool> {
    vec![
        tool(
            "http_capture_query",
            "HTTP captures",
            "Read a bounded page of HTTP capture rows.",
            http_capture_schema(),
        ),
        tool(
            "http_capture_get",
            "HTTP capture detail",
            "Read the complete HTTP capture for an exact session and runtime epoch.",
            object_schema(
                json!({
                    "session_id": {"type": "string", "format": "uuid"},
                    "runtime_epoch": {"type": "string", "format": "uuid"}
                }),
                &["session_id", "runtime_epoch"],
            ),
        ),
        tool(
            "breakpoint_query",
            "Pending breakpoints",
            "Read pending HTTP breakpoints, optionally scoped to one runtime epoch.",
            object_schema(
                json!({"runtime_epoch": {"type": "string", "format": "uuid"}}),
                &[],
            ),
        ),
        tool(
            "breakpoint_get",
            "Breakpoint detail",
            "Read one pending HTTP breakpoint without resolving it.",
            object_schema(
                json!({
                    "breakpoint_id": {"type": "string", "format": "uuid"},
                    "runtime_epoch": {"type": "string", "format": "uuid"}
                }),
                &["breakpoint_id", "runtime_epoch"],
            ),
        ),
    ]
}

fn configuration_tools() -> Vec<Tool> {
    vec![
        tool(
            "http_rule_list",
            "HTTP rules",
            "Read all HTTP rule summaries in runtime order.",
            empty_schema(),
        ),
        tool(
            "http_rule_get",
            "HTTP rule detail",
            "Read one complete HTTP rule.",
            required_uuid("rule_id", "HTTP rule UUID."),
        ),
        tool(
            "protocol_rule_list",
            "Selected Workspace protocol rules",
            "Read all four-stage protocol Document rules for the selected Workspace.",
            empty_schema(),
        ),
        tool(
            "workspace_protocol_rule_list",
            "Workspace protocol rules",
            "Read protocol Document rules from any saved Workspace through its application model.",
            required_uuid("workspace_id", "Workspace UUID."),
        ),
        tool(
            "protocol_package_list",
            "Protocol packages",
            "Read every installed immutable protocol package version and usage count.",
            empty_schema(),
        ),
        tool(
            "protocol_package_catalog",
            "Usable protocol packages",
            "Read enabled, freshly described protocol package capabilities and Schemas.",
            empty_schema(),
        ),
        tool(
            "protocol_package_detail",
            "Protocol package detail",
            "Read one exact package manifest projection, direction capabilities, Schemas and entry usages. Installed source files are intentionally absent from the Application facade.",
            package_schema(),
        ),
        tool(
            "protocol_package_usage",
            "Protocol package usages",
            "Read every saved Workspace/entry reference to one exact package version.",
            package_schema(),
        ),
    ]
}

fn tool(name: &str, title: &str, description: &str, input_schema: Value) -> Tool {
    let Value::Object(input_schema) = input_schema else {
        panic!("static MCP tool input schema must be an object");
    };
    let mut tool = Tool::new(name.to_owned(), description.to_owned(), input_schema)
        .with_title(title.to_owned())
        .with_raw_output_schema(contract::output_schema(name));
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    tool
}

fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}

fn required_string(name: &str, description: &str) -> Value {
    object_schema(
        Value::Object(Map::from_iter([(
            name.to_owned(),
            json!({"type": "string", "description": description}),
        )])),
        &[name],
    )
}

fn required_uuid(name: &str, description: &str) -> Value {
    object_schema(
        Value::Object(Map::from_iter([(
            name.to_owned(),
            json!({"type": "string", "format": "uuid", "description": description}),
        )])),
        &[name],
    )
}

fn package_schema() -> Value {
    object_schema(
        json!({
            "package": {
                "type": "object",
                "additionalProperties": false,
                "properties": {"id": {"type": "string"}, "version": {"type": "string"}},
                "required": ["id", "version"]
            }
        }),
        &["package"],
    )
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    let Value::Object(mut properties) = properties else {
        panic!("static MCP schema properties must be an object");
    };
    contract::describe_properties(&mut properties);
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
        "required": required
    })
}

fn diagnostics_schema() -> Value {
    object_schema(
        json!({
            "keyword": {"type": "string"},
            "after_event_id": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 300}
        }),
        &[],
    )
}

fn application_log_schema() -> Value {
    object_schema(
        json!({
            "level": {
                "type": "string",
                "enum": ["trace", "debug", "info", "warning", "error"]
            },
            "target": {"type": "string"},
            "keyword": {"type": "string"},
            "occurred_from": {"type": "string", "format": "date-time"},
            "occurred_to": {"type": "string", "format": "date-time"},
            "before_log_id": {"type": "integer", "minimum": 1},
            "limit": {"type": "integer", "minimum": 1, "maximum": 500, "default": 200}
        }),
        &[],
    )
}

fn reproduction_report_schema() -> Value {
    object_schema(
        json!({
            "workspace_id": {"type": "string", "format": "uuid"},
            "listener_id": {"type": "string", "format": "uuid"}
        }),
        &["workspace_id", "listener_id"],
    )
}

fn page_properties() -> Map<String, Value> {
    Map::from_iter([
        (
            "page".to_owned(),
            json!({"type": "integer", "minimum": 1, "default": 1}),
        ),
        (
            "page_size".to_owned(),
            json!({"type": "integer", "minimum": 1, "maximum": 200, "default": 100}),
        ),
    ])
}

fn http_capture_schema() -> Value {
    let mut properties = page_properties();
    properties.extend(Map::from_iter([
        ("keyword".to_owned(), json!({"type": "string"})),
        ("terminal_ip".to_owned(), json!({"type": "string"})),
        ("channel".to_owned(), json!({"type": "string"})),
        (
            "stage".to_owned(),
            json!({"type": "string", "enum": ["tls_handshake", "request", "response", "terminal"]}),
        ),
        ("result".to_owned(), json!({"type": "string"})),
        (
            "rule_id".to_owned(),
            json!({"type": "string", "format": "uuid"}),
        ),
        (
            "after_event_id".to_owned(),
            json!({"type": "integer", "minimum": 0}),
        ),
        (
            "sort".to_owned(),
            json!({"type": "string", "enum": ["occurred_at", "terminal_ip", "duration", "size"]}),
        ),
        (
            "direction".to_owned(),
            json!({"type": "string", "enum": ["asc", "desc"]}),
        ),
    ]));
    object_schema(Value::Object(properties), &[])
}

#[cfg(test)]
mod tests;
