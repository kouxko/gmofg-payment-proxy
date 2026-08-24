use std::collections::BTreeSet;

use serde_json::json;

use super::{tools, validate_top_level_arguments};
use crate::mcp::backend::DISPATCHED_TOOL_NAMES;

#[test]
fn every_tool_publishes_a_complete_machine_readable_contract() {
    let tools = tools();
    assert_eq!(tools.len(), 37, "the reference documents all public tools");

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
        assert_eq!(
            tool.input_schema.get("additionalProperties"),
            Some(&json!(false)),
            "{} must publish a closed input contract",
            tool.name
        );
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(|value| value.as_object())
            .expect("tool properties");
        for (field, schema) in properties {
            assert!(
                schema
                    .get("description")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| !value.is_empty()),
                "{}.{field} is missing a description",
                tool.name
            );
        }
        assert!(
            tool.output_schema.is_some(),
            "{} is missing its successful output schema",
            tool.name
        );
    }
}

#[test]
fn closed_tool_inputs_reject_unknown_top_level_fields() {
    validate_top_level_arguments("settings_get", &json!({})).expect("empty input");
    validate_top_level_arguments(
        "workspace_get",
        &json!({"workspace_id": "00000000-0000-0000-0000-000000000000"}),
    )
    .expect("documented input");

    let error = validate_top_level_arguments("settings_get", &json!({"unexpected": true}))
        .expect_err("closed schema must reject extra fields");
    assert!(error.contains("unexpected"));
}
