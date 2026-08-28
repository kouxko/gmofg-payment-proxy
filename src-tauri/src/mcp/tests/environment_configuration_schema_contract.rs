use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::super::environment_contract::{environment_contract_tools, public_literal_registry};
mod catalog;
#[path = "environment_configuration_schema_support.rs"]
mod support;
use support::{
    contains_key, expected_preview, extract_contract_literals, extract_positive_contract_literals,
    literal_array, literal_coverage, published_schemas, schema_snapshot, string_set,
};

#[test]
fn published_input_and_output_schemas_equal_checked_in_snapshot_exactly() {
    assert_eq!(
        published_schemas(),
        schema_snapshot()["tools"],
        "published schemas drifted from the manually authored revision-16 snapshot"
    );
}

#[test]
fn schema_snapshot_is_manual_and_candidate_version_is_const_one() {
    let snapshot = schema_snapshot();
    assert_eq!(snapshot["origin"], "revision16_manual_contract");
    assert_eq!(
        snapshot["tools"]["environment_candidate_create"]["inputSchema"]["$defs"]["candidate"]["properties"]
            ["schema_version"],
        json!({"const": 1})
    );
}

#[test]
fn schema_snapshot_covers_required_unions_enums_and_nullable_fields() {
    let defs = &schema_snapshot()["tools"]["environment_candidate_create"]["inputSchema"]["$defs"];
    assert_eq!(defs["target"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(
        defs["listener"]["properties"]["data_plane"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(defs["documentValue"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(defs["documentAction"]["oneOf"].as_array().unwrap().len(), 4);
    assert_eq!(
        defs["weakNetwork"]["properties"]["burst_loss"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["upload_bytes_per_second"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["download_bytes_per_second"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["path_mtu"]["properties"]["mtu"]["type"],
        json!(["integer", "null"])
    );
    assert_eq!(
        defs["weakNetwork"]["properties"]["path_mtu"]["properties"]["mss_clamp"]["minimum"],
        1
    );
    assert_eq!(
        defs["workspace"]["required"],
        json!(["listeners", "rules", "android_network_profiles"])
    );
    assert!(defs["workspace"]["properties"]["http_rules"].is_null());
    assert!(defs["workspace"]["properties"]["protocol_rules"].is_null());
    assert_eq!(defs["rule"]["oneOf"].as_array().unwrap().len(), 2);
    assert_eq!(
        defs["httpRule"]["properties"]["stage"]["enum"],
        json!([
            "app_to_proxy",
            "proxy_to_upstream",
            "upstream_to_proxy",
            "proxy_to_app",
            "tls_handshake"
        ])
    );
    assert!(
        !defs["httpRule"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("document"))
    );
    for rule in ["httpRule", "socketRule"] {
        assert!(
            !defs[rule]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("existing_rule_id")),
            "{rule}.existing_rule_id must be optional"
        );
    }
}

#[test]
fn every_structured_object_schema_boundary_is_closed_and_declares_required_fields() {
    fn visit(path: &str, value: &Value) {
        match value {
            Value::Object(object) => {
                if object.get("type") == Some(&json!("object")) {
                    if path.ends_with("/$defs/jsonValue/oneOf/5") {
                        assert!(
                            object
                                .get("additionalProperties")
                                .is_some_and(Value::is_object),
                            "recursive JSON object must constrain each value at {path}"
                        );
                    } else {
                        assert!(
                            object.get("properties").is_some(),
                            "object schema lacks properties at {path}"
                        );
                        assert!(
                            object.get("required").is_some(),
                            "object schema lacks required at {path}"
                        );
                        assert_eq!(
                            object.get("additionalProperties"),
                            Some(&json!(false)),
                            "open object at {path}"
                        );
                    }
                }
                if object.get("type") == Some(&json!("array")) {
                    assert!(object.contains_key("items"), "array lacks items at {path}");
                }
                if let Some(types) = object.get("type").and_then(Value::as_array) {
                    assert!(
                        !types.contains(&json!("object")),
                        "nullable object must use oneOf at {path}"
                    );
                }
                for (key, child) in object {
                    visit(&format!("{path}/{key}"), child);
                }
            }
            Value::Array(values) => {
                for (index, child) in values.iter().enumerate() {
                    visit(&format!("{path}/{index}"), child);
                }
            }
            _ => {}
        }
    }
    visit("", &schema_snapshot()["tools"]);
}

#[test]
fn output_common_definitions_are_complete_and_used_by_every_tool() {
    let snapshot = schema_snapshot();
    let tools = snapshot["tools"].as_object().unwrap();
    for name in [
        "environment_candidate_create",
        "environment_candidate_status",
        "environment_candidate_cancel",
        "environment_candidate_apply",
    ] {
        let output = &tools[name]["outputSchema"];
        assert_eq!(
            output["properties"]["errors"]["items"]["$ref"], "#/$defs/diagnostic",
            "{name} errors must use the diagnostic contract"
        );
        for definition in [
            "errorCode",
            "diagnostic",
            "validationLayer",
            "baselinePublic",
            "preview",
            "terminalResult",
        ] {
            assert!(
                !output["$defs"][definition].is_null(),
                "{name}.{definition}"
            );
        }
    }

    let status = &tools["environment_candidate_status"]["outputSchema"];
    assert_eq!(
        status["properties"]["terminal_result"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        status["$defs"]["terminalResult"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        status["$defs"]["preview"]["required"],
        json!([
            "target_key",
            "target",
            "baseline_public",
            "validation_layers",
            "resources",
            "alias_graph",
            "materials_public",
            "protocol_document_values",
            "terminal_action_fields"
        ])
    );
}

#[test]
fn warning_codes_are_disjoint_from_errors_and_only_capabilities_reference_them() {
    let snapshot = schema_snapshot();
    let tools = &snapshot["tools"];
    assert_eq!(
        tools["mcp_environment_capabilities"]["outputSchema"]["$defs"]["warningCode"]["enum"],
        json!([
            "ipv6_unsupported",
            "ipv6_dual_stack_covered",
            "IPV6_DEGRADED"
        ])
    );
    for name in [
        "environment_candidate_create",
        "environment_candidate_status",
        "environment_candidate_cancel",
        "environment_candidate_apply",
    ] {
        let output = &tools[name]["outputSchema"];
        assert!(
            !output["$defs"]["errorCode"]["enum"]
                .as_array()
                .unwrap()
                .contains(&json!("IPV6_DEGRADED")),
            "{name} errorCode contains warning-only IPV6_DEGRADED"
        );
        assert!(!contains_key(output, "warningCode"));
    }
}

#[test]
fn expected_preview_has_exact_public_field_by_field_contract() {
    let preview = expected_preview();
    let expected_root = BTreeSet::from([
        "alias_graph",
        "baseline_public",
        "validation_layers",
        "materials_public",
        "protocol_document_values",
        "resources",
        "target",
        "target_key",
        "terminal_action_fields",
    ]);
    let actual_root: BTreeSet<&str> = preview
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual_root, expected_root);
    assert_eq!(preview["target"], json!({"mode":"new","name":"Store Lab"}));
    assert_eq!(preview["target_key"], "new:53746f7265204c6162");
    assert_eq!(preview["baseline_public"]["workspace_id"], Value::Null);
    assert_eq!(preview["baseline_public"]["revision"], Value::Null);
    assert_eq!(preview["validation_layers"].as_array().unwrap().len(), 7);
    assert_eq!(
        preview["resources"]["listeners"].as_array().unwrap().len(),
        3
    );
    assert!(
        preview["resources"]["listeners"]
            .as_array()
            .unwrap()
            .iter()
            .all(|listener| listener["candidate_local_id"].as_str().is_some())
    );
    assert_eq!(
        preview["resources"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["android_profile_ids", "listeners", "rules"])
    );
    assert_eq!(preview["resources"]["rules"][0]["created_order"], 10);
    for forbidden in [
        "content",
        "password",
        "confirmation_token",
        "protected_bytes",
    ] {
        assert!(
            !contains_key(&preview, forbidden),
            "preview exposes {forbidden}"
        );
    }
}

#[test]
fn every_public_literal_extracted_from_schemas_and_expected_output_is_registered() {
    let registry = public_literal_registry()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut extracted = BTreeSet::new();
    for tool in schema_snapshot()["tools"].as_object().unwrap().values() {
        extract_contract_literals(&tool["outputSchema"], None, &mut extracted);
    }
    for name in environment_contract_tools()
        .into_iter()
        .map(|tool| tool.name.into_owned())
    {
        extracted.insert(name);
    }
    let unregistered = extracted
        .iter()
        .filter(|literal| !registry.contains(literal.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        unregistered.is_empty(),
        "unregistered public literals: {unregistered:?}"
    );
}

#[test]
fn public_literal_coverage_is_bidirectional_and_matches_semantic_schema_categories() {
    let coverage = literal_coverage();
    let registry = public_literal_registry()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let covered = coverage
        .as_object()
        .unwrap()
        .values()
        .flat_map(|values| values.as_array().unwrap())
        .map(|value| value.as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(covered, registry);

    let snapshot = schema_snapshot();
    let tools = &snapshot["tools"];
    assert_eq!(
        coverage["warning_codes"],
        tools["mcp_environment_capabilities"]["outputSchema"]["$defs"]["warningCode"]["enum"]
    );
    assert_eq!(
        string_set(&coverage["error_codes"]),
        string_set(
            &tools["environment_candidate_status"]["outputSchema"]["$defs"]["errorCode"]["enum"]
        )
    );
    assert_eq!(
        coverage["candidate_statuses"],
        tools["environment_candidate_status"]["outputSchema"]["properties"]["status"]["enum"]
    );
    assert_eq!(
        coverage["cancel_statuses"],
        tools["environment_candidate_cancel"]["outputSchema"]["properties"]["status"]["enum"]
    );
    assert_eq!(
        coverage["validation_layers"],
        tools["environment_candidate_status"]["outputSchema"]["$defs"]["validationLayer"]["properties"]
            ["layer"]["enum"]
    );
    assert_eq!(
        coverage["validation_statuses"],
        tools["environment_candidate_status"]["outputSchema"]["$defs"]["validationLayer"]["properties"]
            ["status"]["enum"]
    );
    assert_eq!(
        coverage["severities"],
        tools["environment_candidate_status"]["outputSchema"]["$defs"]["diagnostic"]["properties"]
            ["severity"]["enum"]
    );
    let tool_names = environment_contract_tools()
        .into_iter()
        .map(|tool| tool.name.into_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tool_names,
        string_set(&coverage["tool_names"])
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    let terminal_results =
        tools["environment_candidate_status"]["outputSchema"]["$defs"]["terminalResult"]["oneOf"]
            .as_array()
            .unwrap()
            .iter()
            .map(|variant| variant["properties"]["result"]["const"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
    assert_eq!(terminal_results, string_set(&coverage["terminal_results"]));
}

#[test]
fn all_checked_in_contract_fixtures_have_only_registered_semantic_literals() {
    let fixtures = [
        include_bytes!("fixtures/environment_configuration_candidate_v1/full-shape.json")
            .as_slice(),
        include_bytes!(
            "fixtures/environment_configuration_candidate_v1/existing-target-retained-selector.json"
        )
        .as_slice(),
        include_bytes!("fixtures/environment_configuration_candidate_v1/weak-network-null.json")
            .as_slice(),
        include_bytes!("fixtures/environment_configuration_candidate_v1/negative-cases.json")
            .as_slice(),
        include_bytes!("fixtures/environment_configuration_candidate_v1/expected-preview.json")
            .as_slice(),
        include_bytes!("fixtures/environment_configuration_candidate_v1/literal-coverage.json")
            .as_slice(),
    ];
    let registry = public_literal_registry()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    for fixture in fixtures {
        let value: Value = serde_json::from_slice(fixture).unwrap();
        let mut extracted = BTreeSet::new();
        extract_positive_contract_literals(&value, None, &mut extracted);
        let unknown = extracted.difference(&registry).copied().collect::<Vec<_>>();
        assert!(
            unknown.is_empty(),
            "fixture has unregistered literals: {unknown:?}"
        );
    }
}

#[test]
fn explicit_positive_capability_status_cancel_apply_and_diagnostic_shapes_cover_registry() {
    let coverage = literal_coverage();
    let mut shapes = vec![json!({
        "protocol_version": "environment_configuration_candidate.v1",
        "authentication": "none",
        "source_ip_filter": "none",
        "host_header_policy": "accept_any_syntactically_valid_http_host",
        "origin_policy": "ignored",
        "authorization_policy": "ignored_and_not_required",
        "warnings": coverage["warning_codes"],
        "schema_versions": ["environment_configuration_candidate.v1"],
        "validation_layers": coverage["validation_layers"],
        "terminal_retention": {
            "eviction": "oldest_first",
            "evicted_status_code": "CANDIDATE_NOT_FOUND"
        }
    })];
    shapes.extend(
        literal_array(&coverage, "candidate_statuses")
            .map(|status| json!({"status": status, "expires_on": "app_exit_or_invalidation"})),
    );
    shapes.extend(
        literal_array(&coverage, "cancel_statuses").map(|status| json!({"status": status})),
    );
    shapes.push(json!({"status":"apply_queued"}));
    shapes.extend(
        literal_array(&coverage, "terminal_results").map(|result| json!({"result": result})),
    );
    shapes.extend(
        literal_array(&coverage, "validation_statuses").map(|status| json!({"status": status})),
    );
    shapes.extend(
        literal_array(&coverage, "error_codes")
            .map(|code| json!({"code": code, "status_code": code})),
    );
    shapes.extend(
        literal_array(&coverage, "severities").map(|severity| json!({"severity": severity})),
    );

    let shapes = Value::Array(shapes);
    let tools = environment_contract_tools();
    let mut actual = BTreeSet::new();
    extract_positive_contract_literals(&shapes, None, &mut actual);
    actual.extend(tools.iter().map(|tool| tool.name.as_ref()));
    let expected = public_literal_registry()
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
}
