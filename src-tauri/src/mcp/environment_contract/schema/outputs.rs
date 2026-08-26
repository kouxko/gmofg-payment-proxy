use serde_json::{Map, Value, json};

#[rustfmt::skip]
fn error_code() -> Value { json!({"enum":["AFFECTED_RESOURCE_CHANGED","AFFECTED_RESOURCE_REMOVED","ANDROID_RUNTIME_OWNER_ACTIVE","APPLY_ALREADY_ACTIVE","APPLY_LEASE_MISMATCH","APPLY_LEASE_UNAVAILABLE","CANDIDATE_CANCELLED","CANDIDATE_CANCELLED_BY_SHUTDOWN","CANDIDATE_CAPACITY_EXCEEDED","CANDIDATE_NOT_FOUND","CANDIDATE_STALE","CERTIFICATE_PARSE_FAILED","CERTIFICATE_ROLE_MISMATCH","COMMIT_BASELINE_MISMATCH","COMMIT_FAILED","COMMIT_ROLLED_BACK","CONFIRMATION_TOKEN_INVALID","CONFIRMATION_TOKEN_MISSING","DOCUMENT_VALUE_WIRE_INVALID","DTO_LIMIT_EXCEEDED","EXISTING_RULE_ID_BINDING_MISMATCH","EXISTING_RULE_ID_DUPLICATE","EXISTING_RULE_ID_FORBIDDEN","EXISTING_RULE_ID_KIND_MISMATCH","EXISTING_RULE_ID_PACKAGE_MISMATCH","EXISTING_RULE_ID_SCHEMA_VERSION_MISMATCH","EXISTING_RULE_ID_STAGE_MISMATCH","EXISTING_RULE_ID_UNKNOWN","EXISTING_RULE_ID_WORKSPACE_MISMATCH","EXTERNAL_PACKAGE_OFFLINE","FORBIDDEN_FIELD","HARD_KILL_STATUS_UNAVAILABLE","HTTP_BODY_TOO_LARGE","HTTP_MALFORMED","HTTP_METHOD_NOT_ALLOWED","HTTP_PATH_NOT_FOUND","HTTP_RULE_INVALID","INVALID_PROTOCOL_PACKAGE_VERSION","IPV4_BIND_FAILED","LISTENER_ALIAS_DUPLICATE","LISTENER_ALIAS_MISSING","LISTENER_ALIAS_TYPE_MISMATCH","LISTENER_DOMAIN_INVALID","MATERIAL_ALIAS_DUPLICATE","MATERIAL_ALIAS_MISSING","MATERIAL_ALIAS_MULTIPLE_CONSUMERS_UNSUPPORTED","MATERIAL_ALIAS_TYPE_MISMATCH","MATERIAL_ALIAS_UNUSED","MCP_CREATE_DEADLINE_EXCEEDED","MCP_PROTOCOL_INVALID","MCP_TOOL_ARGUMENTS_INVALID","PROTECTED_MATERIAL_PREPARE_FAILED","PROTOCOL_DOCUMENT_RULE_INVALID","PROTOCOL_PACKAGE_DISABLED","PROTOCOL_PACKAGE_INCOMPATIBLE","PROTOCOL_PACKAGE_NOT_INSTALLED","RUNTIME_ACTIVE","SCHEMA_INVALID","SECRET_VALUE_INVALID","SHUTDOWN_IN_PROGRESS","TARGET_CANDIDATE_ALREADY_ACTIVE","TOKEN_CONSUMED","UNKNOWN_FIELD","UNSUPPORTED_MATERIAL_ROLE","UNSUPPORTED_SECRET_ROLE","VALIDATION_LAYER_FAILED","WEAK_NETWORK_VALUE_INVALID","WEAK_NETWORK_WIRE_INVALID","WORKSPACE_NAME_COLLISION","WORKSPACE_NAME_EMPTY"]}) }

#[rustfmt::skip]
fn diagnostic() -> Value { json!({"type":"object","additionalProperties":false,"required":["code","field","message","severity"],"properties":{"code":{"$ref":"#/$defs/errorCode"},"field":{"type":["string","null"]},"message":{"type":"string"},"severity":{"enum":["error","warning","info"]}}}) }

#[rustfmt::skip]
fn validation_layer() -> Value { json!({"type":"object","additionalProperties":false,"required":["layer","status","code","reason","duration_ms"],"properties":{"layer":{"enum":["schema","domain","material","package_projection","dns_tcp_port","tls_mtls","preview_baseline"]},"status":{"enum":["passed","failed","cancelled","not_applicable","skipped_dependency"]},"code":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/errorCode"}]},"reason":{"type":["string","null"]},"duration_ms":{"type":"integer","minimum":0}}}) }

#[rustfmt::skip]
fn baseline_public() -> Value { json!({"type":"object","additionalProperties":false,"required":["workspace_id","revision","selected"],"properties":{"workspace_id":{"type":["string","null"]},"revision":{"type":["integer","null"],"minimum":0},"selected":{"type":"boolean"}}}) }

#[rustfmt::skip]
fn preview() -> Value { json!({"type":"object","additionalProperties":false,"required":["target_key","target","baseline_public","validation_layers","resources","alias_graph","materials_public","protocol_document_values","terminal_action_fields"],"properties":{"target_key":{"type":"string"},"target":{"oneOf":[{"type":"object","additionalProperties":false,"required":["mode","workspace_id","expected_revision"],"properties":{"mode":{"const":"existing"},"workspace_id":{"type":"string","format":"uuid"},"expected_revision":{"type":"integer","minimum":0}}},{"type":"object","additionalProperties":false,"required":["mode","name"],"properties":{"mode":{"const":"new"},"name":{"type":"string"}}}]},"baseline_public":{"type":"object","additionalProperties":false,"required":["workspace_id","revision","selected"],"properties":{"workspace_id":{"type":["string","null"]},"revision":{"type":["integer","null"],"minimum":0},"selected":{"type":"boolean"}}},"resources":{"type":"object","additionalProperties":false,"required":["listeners","http_rules","protocol_rules","android_profile_ids"],"properties":{"listeners":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["alias","candidate_local_id"],"properties":{"alias":{"type":"string"},"candidate_local_id":{"type":"string","format":"uuid"}}}},"http_rules":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["candidate_index","candidate_local_id","created_order","listener_alias"],"properties":{"candidate_index":{"type":"integer","minimum":0},"candidate_local_id":{"type":"string","format":"uuid"},"created_order":{"type":"integer","minimum":0},"listener_alias":{"type":"string"}}}},"protocol_rules":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["candidate_index","candidate_local_id","created_order","listener_alias"],"properties":{"candidate_index":{"type":"integer","minimum":0},"candidate_local_id":{"type":"string","format":"uuid"},"created_order":{"type":"integer","minimum":0},"listener_alias":{"type":"string"}}}},"android_profile_ids":{"type":"array","items":{"type":"string"}}}},"alias_graph":{"type":"object","additionalProperties":false,"required":["certificate_aliases","secret_aliases"],"properties":{"certificate_aliases":{"type":"array","items":{"type":"string"}},"secret_aliases":{"type":"array","items":{"type":"string"}}}},"materials_public":{"type":"object","additionalProperties":false,"required":["certificates","secrets"],"properties":{"certificates":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["alias","role","encoding","label"],"properties":{"alias":{"type":"string"},"role":{"enum":["downstream_server_identity","downstream_client_trust","upstream_client_identity","upstream_server_trust"]},"encoding":{"enum":["pem","base64_der","pkcs12_base64"]},"label":{"type":"string"}}}},"secrets":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["alias","role","username","label"],"properties":{"alias":{"type":"string"},"role":{"const":"proxy_basic_auth"},"username":{"type":"string"},"label":{"type":"string"}}}}}},"protocol_document_values":{"type":"array","items":{"oneOf":[{"type":"object","additionalProperties":false,"required":["type","value"],"properties":{"type":{"const":"string"},"value":{"type":"string"}}},{"type":"object","additionalProperties":false,"required":["type","value"],"properties":{"type":{"const":"int"},"value":{"type":"integer"}}},{"type":"object","additionalProperties":false,"required":["type","value"],"properties":{"type":{"const":"bool"},"value":{"type":"boolean"}}},{"type":"object","additionalProperties":false,"required":["type","value"],"properties":{"type":{"const":"blob"},"value":{"type":"array","items":{"type":"integer","minimum":0,"maximum":255}}}}]}},"terminal_action_fields":{"type":"object","additionalProperties":false,"required":["TruncateResponse","DisconnectDuringUpstreamWrite","DisconnectDuringDownstreamWrite"],"properties":{"TruncateResponse":{"const":["bytes"]},"DisconnectDuringUpstreamWrite":{"const":["after_bytes"]},"DisconnectDuringDownstreamWrite":{"const":["after_bytes"]}}},"validation_layers":{"type":"array","items":{"$ref":"#/$defs/validationLayer"}}}}) }

#[rustfmt::skip]
fn terminal_result() -> Value { json!({"oneOf":[{"type":"object","additionalProperties":false,"required":["result","workspace_id","revision","selected_workspace_id","apply_task_id","status_code","diagnostics"],"properties":{"result":{"const":"committed"},"workspace_id":{"type":"string"},"revision":{"type":"integer","minimum":0},"selected_workspace_id":{"type":["string","null"]},"apply_task_id":{"type":["string","null"]},"status_code":{"type":"null"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"validation_failed"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"stale"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"cancelled"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"cancelled_by_shutdown"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"failed_before_commit"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}},{"type":"object","additionalProperties":false,"required":["result","status_code","diagnostics"],"properties":{"result":{"const":"rolled_back"},"status_code":{"$ref":"#/$defs/errorCode"},"diagnostics":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}}]}) }

fn common_definitions() -> Map<String, Value> {
    [
        ("errorCode".to_owned(), error_code()),
        ("diagnostic".to_owned(), diagnostic()),
        ("validationLayer".to_owned(), validation_layer()),
        ("baselinePublic".to_owned(), baseline_public()),
        ("preview".to_owned(), preview()),
        ("terminalResult".to_owned(), terminal_result()),
    ]
    .into_iter()
    .collect()
}

fn with_common_definitions(mut schema: Value) -> Value {
    schema
        .as_object_mut()
        .expect("environment output root is an object")
        .insert("$defs".to_owned(), Value::Object(common_definitions()));
    schema
}

#[rustfmt::skip]
fn create_root() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id","confirmation_token","status","target_key","baseline_public","validation_layers","preview","expires_on","errors"],"properties":{"candidate_id":{"type":"string"},"confirmation_token":{"type":["string","null"]},"status":{"enum":["preview_ready","validation_failed","cancelled","cancelled_by_shutdown"]},"target_key":{"type":"string"},"baseline_public":{"$ref":"#/$defs/baselinePublic"},"validation_layers":{"type":"array","items":{"$ref":"#/$defs/validationLayer"}},"preview":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/preview"}]},"expires_on":{"const":"app_exit_or_invalidation"},"errors":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}}) }

pub(super) fn create_schema() -> Value {
    with_common_definitions(create_root())
}

#[rustfmt::skip]
fn status_root() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id","status","target_key","baseline_public","validation_layers","preview","terminal_result","errors"],"properties":{"candidate_id":{"type":"string"},"status":{"enum":["validating","preview_ready","validation_failed","stale","cancelled","cancelled_by_shutdown","apply_queued","apply_in_progress","committed","rolled_back","failed_before_commit","not_found"]},"target_key":{"type":["string","null"]},"baseline_public":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/baselinePublic"}]},"validation_layers":{"type":"array","items":{"$ref":"#/$defs/validationLayer"}},"preview":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/preview"}]},"terminal_result":{"oneOf":[{"type":"null"},{"$ref":"#/$defs/terminalResult"}]},"errors":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}}) }

pub(super) fn status_schema() -> Value {
    with_common_definitions(status_root())
}

#[rustfmt::skip]
fn cancel_root() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id","status","terminal","errors"],"properties":{"candidate_id":{"type":"string"},"status":{"enum":["cancelled","apply_in_progress_not_cancellable","not_found_or_terminal"]},"terminal":{"type":"boolean"},"errors":{"type":"array","items":{"$ref":"#/$defs/diagnostic"}}}}) }

pub(super) fn cancel_schema() -> Value {
    with_common_definitions(cancel_root())
}

#[rustfmt::skip]
fn apply_root() -> Value { json!({"type":"object","additionalProperties":false,"required":["candidate_id","apply_task_id","status","errors"],"properties":{"candidate_id":{"type":"string"},"apply_task_id":{"type":"string"},"status":{"const":"apply_queued"},"errors":{"type":"array","items":{"$ref":"#/$defs/diagnostic"},"maxItems":0}}}) }

pub(super) fn apply_schema() -> Value {
    with_common_definitions(apply_root())
}
