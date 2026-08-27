use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{tools, validate_arguments};
use crate::mcp::backend::DISPATCHED_TOOL_NAMES;

fn assert_closed_described_objects(tool_name: &str, path: &str, schema: &serde_json::Value) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    let is_object = schema.get("type") == Some(&json!("object"));
    if is_object {
        assert_eq!(
            schema.get("additionalProperties"),
            Some(&json!(false)),
            "{tool_name}.{path} must publish a closed object contract"
        );
    }
    let Some(properties) = schema.get("properties").and_then(|value| value.as_object()) else {
        return;
    };
    for (field, child) in properties {
        let child_path = if path.is_empty() {
            field.clone()
        } else {
            format!("{path}.{field}")
        };
        assert!(
            child
                .get("description")
                .and_then(|value| value.as_str())
                .is_some_and(|value| !value.is_empty()),
            "{tool_name}.{child_path} is missing a description"
        );
        assert_closed_described_objects(tool_name, &child_path, child);
    }
}

#[test]
fn every_tool_publishes_a_complete_machine_readable_contract() {
    let tools = tools();
    let array_results = [
        "workspace_list",
        "entry_status_list",
        "android_device_list",
        "android_package_list",
        "android_runtime_owner_list",
        "android_profile_list",
        "workspace_certificate_overview",
        "breakpoint_query",
        "http_rule_list",
        "protocol_rule_list",
        "workspace_protocol_rule_list",
        "protocol_package_list",
        "protocol_package_usage",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(
        tools.len(),
        42,
        "37 existing reads plus five environment tools"
    );

    let names = tools
        .iter()
        .map(|tool| tool.name.as_ref())
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), tools.len(), "tool names must be unique");
    assert_eq!(
        names,
        DISPATCHED_TOOL_NAMES.iter().copied().collect(),
        "catalog and backend dispatcher must expose the same tools"
    );

    for tool in tools {
        assert!(
            tool.description
                .as_ref()
                .is_some_and(|value| !value.is_empty()),
            "{} is missing its purpose",
            tool.name
        );
        assert_closed_described_objects(
            tool.name.as_ref(),
            "",
            &Value::Object((*tool.input_schema).clone()),
        );
        let output_schema = tool
            .output_schema
            .as_ref()
            .unwrap_or_else(|| panic!("{} is missing its successful output schema", tool.name));
        let expected_type = if array_results.contains(tool.name.as_ref()) {
            json!("array")
        } else {
            json!("object")
        };
        assert_eq!(
            output_schema.get("type"),
            Some(&expected_type),
            "{} publishes the wrong successful result root",
            tool.name
        );
    }
}

#[test]
fn closed_tool_inputs_reject_unknown_top_level_fields() {
    validate_arguments("settings_get", &json!({})).expect("empty input");
    validate_arguments(
        "workspace_get",
        &json!({"workspace_id": "00000000-0000-0000-0000-000000000000"}),
    )
    .expect("documented input");

    let error = validate_arguments("settings_get", &json!({"unexpected": true}))
        .expect_err("closed schema must reject extra fields");
    assert!(error.contains("unexpected"));
}

#[test]
fn unknown_tool_error_uses_the_general_mcp_catalog_wording() {
    let error = validate_arguments("not_registered", &json!({}))
        .expect_err("an unknown tool must be rejected");
    assert_eq!(error, "unknown MCP tool: not_registered");
}

#[test]
fn closed_tool_inputs_reject_unknown_nested_fields() {
    let error = validate_arguments(
        "exchange_observation_query",
        &json!({
            "workspace_id": "00000000-0000-0000-0000-000000000000",
            "page": {"page": 1, "page_size": 50, "unexpected": true}
        }),
    )
    .expect_err("closed nested schema must reject extra fields");
    assert!(error.contains("page.unexpected"), "{error}");
}

#[test]
fn published_schema_constraints_are_enforced_for_all_input_levels() {
    for (name, arguments, field) in [
        ("workspace_get", json!({}), "workspace_id"),
        (
            "workspace_get",
            json!({"workspace_id": "not-a-uuid"}),
            "workspace_id",
        ),
        ("diagnostics_query", json!({"limit": 501}), "limit"),
        ("application_log_query", json!({"level": "fatal"}), "level"),
        (
            "exchange_observation_query",
            json!({
                "workspace_id": "00000000-0000-0000-0000-000000000000",
                "page": {"page": 1}
            }),
            "page.page_size",
        ),
        (
            "protocol_package_detail",
            json!({"package": {"id": "example"}}),
            "package.version",
        ),
        ("android_package_list", json!({}), "serial"),
        (
            "android_package_get",
            json!({"package_name": "com.example"}),
            "serial",
        ),
        ("android_network_status", json!({}), "serial"),
        ("android_network_endpoints", json!({}), "serial"),
    ] {
        let error = validate_arguments(name, &arguments)
            .err()
            .unwrap_or_else(|| panic!("{name}.{field} must reject arguments {arguments}"));
        assert!(error.contains(field), "{name}: {error}");
    }
}

#[test]
fn retired_single_android_owner_tool_is_not_registered() {
    assert_eq!(
        validate_arguments("android_runtime_owner", &json!({})),
        Err("unknown MCP tool: android_runtime_owner".into())
    );
}
