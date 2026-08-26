//! Contract-only registry for the staged MCP environment configuration workflow.
//!
//! These tools are deliberately not merged into the active catalog or dispatch path. G036 owns
//! runtime exposure after the Application lifecycle and persistence behavior exists.

mod schema;

use std::collections::BTreeSet;

use intercept_proxy_application::parse_environment_configuration_candidate_v1;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::Value;

pub fn environment_contract_tools() -> Vec<Tool> {
    vec![
        tool(
            "mcp_environment_capabilities",
            "Environment configuration capabilities",
            "Describe the staged environment configuration contract.",
            contract_schema("mcp_environment_capabilities", "inputSchema"),
            contract_schema("mcp_environment_capabilities", "outputSchema"),
            (true, false, true),
        ),
        tool(
            "environment_candidate_create",
            "Create environment candidate",
            "Parse and validate one complete environment configuration candidate.",
            contract_schema("environment_candidate_create", "inputSchema"),
            contract_schema("environment_candidate_create", "outputSchema"),
            (false, false, false),
        ),
        tool(
            "environment_candidate_status",
            "Environment candidate status",
            "Read one candidate's process-local status.",
            contract_schema("environment_candidate_status", "inputSchema"),
            contract_schema("environment_candidate_status", "outputSchema"),
            (true, false, true),
        ),
        tool(
            "environment_candidate_cancel",
            "Cancel environment candidate",
            "Cancel one candidate before apply owns its commit work.",
            contract_schema("environment_candidate_cancel", "inputSchema"),
            contract_schema("environment_candidate_cancel", "outputSchema"),
            (false, true, true),
        ),
        tool(
            "environment_candidate_apply",
            "Apply environment candidate",
            "Consume one confirmation token and queue the candidate for Application-owned apply.",
            contract_schema("environment_candidate_apply", "inputSchema"),
            contract_schema("environment_candidate_apply", "outputSchema"),
            (false, true, false),
        ),
    ]
}

pub fn validate_environment_contract_arguments(
    name: &str,
    arguments: &Value,
) -> Result<(), String> {
    let object = arguments
        .as_object()
        .ok_or_else(|| "environment tool arguments must be an object".to_owned())?;
    let expected = match name {
        "mcp_environment_capabilities" => &[][..],
        "environment_candidate_create" => &["candidate"][..],
        "environment_candidate_status" | "environment_candidate_cancel" => &["candidate_id"][..],
        "environment_candidate_apply" => &["candidate_id", "confirmation_token"][..],
        _ => return Err(format!("unknown environment contract tool: {name}")),
    };
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(format!("invalid top-level arguments for {name}"));
    }

    if name == "environment_candidate_create" {
        let candidate = object
            .get("candidate")
            .expect("candidate key was checked above");
        let bytes = serde_json::to_vec(candidate).map_err(|error| error.to_string())?;
        parse_environment_configuration_candidate_v1(&bytes).map_err(|error| error.to_string())?;
    } else {
        for field in expected {
            if object.get(field).and_then(Value::as_str).is_none() {
                return Err(format!("{name}.{field} must be a string"));
            }
        }
    }
    Ok(())
}

pub fn public_literal_registry() -> &'static [&'static str] {
    PUBLIC_LITERALS
}

const PUBLIC_LITERALS: &[&str] = &[
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
    "EXISTING_RULE_ID_SCHEMA_VERSION_MISMATCH",
    "EXISTING_RULE_ID_STAGE_MISMATCH",
    "HTTP_RULE_INVALID",
    "PROTOCOL_DOCUMENT_RULE_INVALID",
    "DOCUMENT_VALUE_WIRE_INVALID",
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
    "IPV4_BIND_FAILED",
    "HTTP_METHOD_NOT_ALLOWED",
    "HTTP_PATH_NOT_FOUND",
    "HTTP_BODY_TOO_LARGE",
    "HTTP_MALFORMED",
    "MCP_PROTOCOL_INVALID",
    "MCP_TOOL_ARGUMENTS_INVALID",
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

const _: fn() -> Vec<Tool> = environment_contract_tools;
const _: fn(&str, &Value) -> Result<(), String> = validate_environment_contract_arguments;
const _: fn() -> &'static [&'static str] = public_literal_registry;

fn tool(
    name: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    annotations: (bool, bool, bool),
) -> Tool {
    let Value::Object(input_schema) = input_schema else {
        unreachable!("environment input schemas are objects")
    };
    let Value::Object(output_schema) = output_schema else {
        unreachable!("environment output schemas are objects")
    };
    let mut tool = Tool::new(name.to_owned(), description.to_owned(), input_schema)
        .with_title(title.to_owned())
        .with_raw_output_schema(output_schema.into());
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(annotations.0)
            .destructive(annotations.1)
            .idempotent(annotations.2)
            .open_world(false),
    );
    tool
}

fn contract_schema(tool_name: &str, schema_name: &str) -> Value {
    schema::document()["tools"][tool_name][schema_name].clone()
}
