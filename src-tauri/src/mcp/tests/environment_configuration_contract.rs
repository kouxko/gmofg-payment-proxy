use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::super::environment_contract::{
    environment_contract_tools, public_literal_registry, validate_environment_contract_arguments,
};

#[derive(Clone, Copy)]
struct ExpectedAnnotation {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
}

fn expected_tools() -> BTreeMap<&'static str, ExpectedAnnotation> {
    BTreeMap::from([
        (
            "mcp_environment_capabilities",
            ExpectedAnnotation {
                read_only: true,
                destructive: false,
                idempotent: true,
            },
        ),
        (
            "environment_candidate_create",
            ExpectedAnnotation {
                read_only: false,
                destructive: false,
                idempotent: false,
            },
        ),
        (
            "environment_candidate_status",
            ExpectedAnnotation {
                read_only: true,
                destructive: false,
                idempotent: true,
            },
        ),
        (
            "environment_candidate_cancel",
            ExpectedAnnotation {
                read_only: false,
                destructive: true,
                idempotent: true,
            },
        ),
        (
            "environment_candidate_apply",
            ExpectedAnnotation {
                read_only: false,
                destructive: true,
                idempotent: false,
            },
        ),
    ])
}

#[test]
fn environment_contract_registry_has_exact_mixed_tool_annotations() {
    let tools = environment_contract_tools();
    let expected = expected_tools();

    assert_eq!(tools.len(), expected.len());
    for tool in tools {
        let annotation = tool.annotations.as_ref().expect("tool annotations");
        let expected = expected
            .get(tool.name.as_ref())
            .expect("registered tool name");
        assert_eq!(
            annotation.read_only_hint,
            Some(expected.read_only),
            "{}",
            tool.name
        );
        assert_eq!(
            annotation.destructive_hint,
            Some(expected.destructive),
            "{}",
            tool.name
        );
        assert_eq!(
            annotation.idempotent_hint,
            Some(expected.idempotent),
            "{}",
            tool.name
        );
    }
}

#[test]
fn environment_contract_registry_publishes_closed_input_and_output_schemas() {
    for tool in environment_contract_tools() {
        let input = Value::Object((*tool.input_schema).clone());
        let output = Value::Object(
            (**tool
                .output_schema
                .as_ref()
                .expect("environment output schema"))
            .clone(),
        );

        assert_closed_object(&tool.name, "input", &input);
        assert_closed_object(&tool.name, "output", &output);
    }
}

#[test]
fn environment_contract_registry_publishes_exact_top_level_fields() {
    let expected_inputs = BTreeMap::from([
        ("mcp_environment_capabilities", &[][..]),
        ("environment_candidate_create", &["candidate"][..]),
        ("environment_candidate_status", &["candidate_id"][..]),
        ("environment_candidate_cancel", &["candidate_id"][..]),
        (
            "environment_candidate_apply",
            &["candidate_id", "confirmation_token"][..],
        ),
    ]);
    let expected_outputs = BTreeMap::from([
        (
            "mcp_environment_capabilities",
            &[
                "authentication",
                "authorization_policy",
                "candidate_limits",
                "endpoint",
                "host_header_policy",
                "ipv4",
                "ipv6",
                "origin_policy",
                "plaintext_http",
                "protocol_version",
                "read_budgets",
                "schema_versions",
                "source_ip_filter",
                "terminal_retention",
                "validation_layers",
                "warnings",
                "write_budgets",
            ][..],
        ),
        (
            "environment_candidate_create",
            &[
                "baseline_public",
                "candidate_id",
                "confirmation_token",
                "errors",
                "expires_on",
                "preview",
                "status",
                "target_key",
                "validation_layers",
            ][..],
        ),
        (
            "environment_candidate_status",
            &[
                "baseline_public",
                "candidate_id",
                "errors",
                "preview",
                "status",
                "target_key",
                "terminal_result",
                "validation_layers",
            ][..],
        ),
        (
            "environment_candidate_cancel",
            &["candidate_id", "errors", "status", "terminal"][..],
        ),
        (
            "environment_candidate_apply",
            &["apply_task_id", "candidate_id", "errors", "status"][..],
        ),
    ]);

    for tool in environment_contract_tools() {
        let output = tool
            .output_schema
            .as_ref()
            .expect("environment output schema");
        assert_eq!(
            property_names(tool.input_schema.as_ref()),
            expected_inputs[tool.name.as_ref()]
                .iter()
                .copied()
                .collect(),
            "{} input fields",
            tool.name
        );
        assert_eq!(
            property_names(output.as_ref()),
            expected_outputs[tool.name.as_ref()]
                .iter()
                .copied()
                .collect(),
            "{} output fields",
            tool.name
        );
    }
}

#[test]
fn environment_create_schema_accepts_only_the_canonical_full_shape_fixture() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/full-shape.json"
    ))
    .unwrap();
    validate_environment_contract_arguments(
        "environment_candidate_create",
        &json!({"candidate": fixture}),
    )
    .expect("canonical full-shape fixture satisfies published schema");
}

#[test]
fn environment_create_rejects_legacy_split_rule_collections_with_stable_error() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "fixtures/environment_configuration_candidate_v1/full-shape.json"
    ))
    .unwrap();

    for legacy_field in ["http_rules", "protocol_rules"] {
        let mut candidate = fixture.clone();
        candidate["workspace"]
            .as_object_mut()
            .unwrap()
            .insert(legacy_field.to_owned(), json!([]));
        assert_eq!(
            validate_environment_contract_arguments(
                "environment_candidate_create",
                &json!({"candidate": candidate}),
            ),
            Err("environment candidate violates the published schema".to_owned()),
            "legacy workspace.{legacy_field} must remain rejected"
        );
    }
}

#[test]
fn environment_public_literal_registry_is_closed_and_complete() {
    let registry = public_literal_registry();
    let expected = expected_public_literals();
    let actual = registry.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}

const CONTRACT_AND_POLICY_LITERALS: &[&str] = &[
    "environment_configuration_candidate.v1",
    "app_exit_or_invalidation",
    "mcp_environment_capabilities",
    "environment_candidate_create",
    "environment_candidate_status",
    "environment_candidate_cancel",
    "environment_candidate_apply",
    "none",
    "accept_any_syntactically_valid_http_host",
    "ignored",
    "ignored_and_not_required",
    "ipv6_unsupported",
    "ipv6_dual_stack_covered",
    "IPV6_DEGRADED",
    "oldest_first",
];

const VALIDATION_LITERALS: &[&str] = &[
    "SCHEMA_INVALID",
    "UNKNOWN_FIELD",
    "FORBIDDEN_FIELD",
    "DTO_LIMIT_EXCEEDED",
    "WORKSPACE_NAME_EMPTY",
    "WORKSPACE_NAME_COLLISION",
    "LISTENER_ALIAS_DUPLICATE",
    "LISTENER_ALIAS_MISSING",
    "LISTENER_ALIAS_TYPE_MISMATCH",
    "LISTENER_DOMAIN_INVALID",
    "EXISTING_RULE_ID_FORBIDDEN",
    "EXISTING_RULE_ID_UNKNOWN",
    "EXISTING_RULE_ID_DUPLICATE",
    "EXISTING_RULE_ID_WORKSPACE_MISMATCH",
    "EXISTING_RULE_ID_KIND_MISMATCH",
    "EXISTING_RULE_ID_BINDING_MISMATCH",
    "EXISTING_RULE_ID_PACKAGE_MISMATCH",
    "EXISTING_RULE_ID_STAGE_MISMATCH",
    "HTTP_RULE_INVALID",
    "PROTOCOL_DOCUMENT_RULE_INVALID",
    "WEAK_NETWORK_WIRE_INVALID",
    "WEAK_NETWORK_VALUE_INVALID",
    "MATERIAL_ALIAS_DUPLICATE",
    "MATERIAL_ALIAS_MISSING",
    "MATERIAL_ALIAS_TYPE_MISMATCH",
    "MATERIAL_ALIAS_UNUSED",
    "MATERIAL_ALIAS_MULTIPLE_CONSUMERS_UNSUPPORTED",
    "UNSUPPORTED_SECRET_ROLE",
    "UNSUPPORTED_MATERIAL_ROLE",
    "CERTIFICATE_PARSE_FAILED",
    "CERTIFICATE_ROLE_MISMATCH",
    "SECRET_VALUE_INVALID",
    "INVALID_PROTOCOL_PACKAGE_VERSION",
    "PROTOCOL_PACKAGE_NOT_INSTALLED",
    "PROTOCOL_PACKAGE_DISABLED",
    "EXTERNAL_PACKAGE_OFFLINE",
    "PROTOCOL_PACKAGE_INCOMPATIBLE",
    "MCP_CREATE_DEADLINE_EXCEEDED",
    "VALIDATION_LAYER_FAILED",
];

const LIFECYCLE_LITERALS: &[&str] = &[
    "CANDIDATE_NOT_FOUND",
    "CANDIDATE_STALE",
    "CANDIDATE_CANCELLED",
    "CANDIDATE_CANCELLED_BY_SHUTDOWN",
    "CANDIDATE_CAPACITY_EXCEEDED",
    "TARGET_CANDIDATE_ALREADY_ACTIVE",
    "APPLY_ALREADY_ACTIVE",
    "CONFIRMATION_TOKEN_MISSING",
    "CONFIRMATION_TOKEN_INVALID",
    "TOKEN_CONSUMED",
    "SHUTDOWN_IN_PROGRESS",
    "RUNTIME_ACTIVE",
    "ANDROID_RUNTIME_OWNER_ACTIVE",
    "AFFECTED_RESOURCE_CHANGED",
    "AFFECTED_RESOURCE_REMOVED",
    "APPLY_LEASE_UNAVAILABLE",
    "APPLY_LEASE_MISMATCH",
    "PROTECTED_MATERIAL_PREPARE_FAILED",
    "COMMIT_BASELINE_MISMATCH",
    "COMMIT_ROLLED_BACK",
    "COMMIT_FAILED",
    "HARD_KILL_STATUS_UNAVAILABLE",
];

const TRANSPORT_LITERALS: &[&str] = &[
    "IPV4_BIND_FAILED",
    "HTTP_METHOD_NOT_ALLOWED",
    "HTTP_PATH_NOT_FOUND",
    "HTTP_BODY_TOO_LARGE",
    "HTTP_MALFORMED",
    "MCP_PROTOCOL_INVALID",
    "MCP_TOOL_ARGUMENTS_INVALID",
];

const VALIDATION_STATE_LITERALS: &[&str] = &[
    "schema",
    "domain",
    "material",
    "package_projection",
    "dns_tcp_port",
    "tls_mtls",
    "preview_baseline",
    "passed",
    "failed",
    "cancelled",
    "not_applicable",
    "skipped_dependency",
];

const CANDIDATE_AND_RESULT_LITERALS: &[&str] = &[
    "validating",
    "preview_ready",
    "validation_failed",
    "stale",
    "cancelled_by_shutdown",
    "apply_queued",
    "apply_in_progress",
    "committed",
    "rolled_back",
    "failed_before_commit",
    "not_found",
    "apply_in_progress_not_cancellable",
    "not_found_or_terminal",
    "error",
    "warning",
    "info",
];

fn expected_public_literals() -> BTreeSet<&'static str> {
    [
        CONTRACT_AND_POLICY_LITERALS,
        VALIDATION_LITERALS,
        LIFECYCLE_LITERALS,
        TRANSPORT_LITERALS,
        VALIDATION_STATE_LITERALS,
        CANDIDATE_AND_RESULT_LITERALS,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect()
}

fn property_names(schema: &serde_json::Map<String, Value>) -> BTreeSet<&str> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("object schema properties")
        .keys()
        .map(String::as_str)
        .collect()
}

fn assert_closed_object(tool: &str, path: &str, schema: &Value) {
    let Some(object) = schema.as_object() else {
        return;
    };
    if object.get("type") == Some(&json!("object")) {
        assert_eq!(
            object.get("additionalProperties"),
            Some(&json!(false)),
            "{tool}.{path} must be closed"
        );
    }
    if let Some(properties) = object.get("properties").and_then(Value::as_object) {
        for (field, child) in properties {
            assert_closed_object(tool, &format!("{path}.{field}"), child);
        }
    }
    for keyword in ["items", "oneOf", "anyOf", "allOf"] {
        match object.get(keyword) {
            Some(Value::Array(children)) => {
                for (index, child) in children.iter().enumerate() {
                    assert_closed_object(tool, &format!("{path}.{keyword}[{index}]"), child);
                }
            }
            Some(child) => assert_closed_object(tool, &format!("{path}.{keyword}"), child),
            None => {}
        }
    }
}
