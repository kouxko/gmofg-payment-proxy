use serde_json::{Value, json};

use super::{
    EnvironmentCandidatePublicSnapshot, EnvironmentConfigurationCandidateV1, EnvironmentTarget,
    EnvironmentValidationResult, lifecycle::exact_public_target_key,
};
use crate::{AppError, AppResult};
use intercept_proxy_domain::ProxyWorkspace;

pub(crate) fn candidate_preview_snapshot(
    candidate: &EnvironmentConfigurationCandidateV1,
    prior_layers: &[EnvironmentValidationResult],
    projected_workspace: &ProxyWorkspace,
) -> AppResult<EnvironmentCandidatePublicSnapshot> {
    let projected_workspace =
        serde_json::to_value(projected_workspace).map_err(|_| preview_failure())?;
    let validation_layers = validation_layers_with_preview(prior_layers)?;

    let listeners = candidate
        .workspace
        .listeners
        .iter()
        .enumerate()
        .map(|(index, listener)| {
            let alias = listener.alias();
            let id = string_field(&projected_workspace["listeners"][index], "id")?;
            Ok(json!({ "alias": alias, "candidate_local_id": id }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let http_rules = candidate
        .workspace
        .http_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let projected = &projected_workspace["rules"][index];
            Ok(json!({
                "candidate_index": index,
                "candidate_local_id": string_field(projected, "id")?,
                "created_order": integer_field(projected, "created_order")?,
                "listener_alias": rule.listener_alias()
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let protocol_rules = candidate
        .workspace
        .protocol_rules
        .iter()
        .enumerate()
        .map(|(index, rule)| {
            let projected = &projected_workspace["protocol_rules"][index];
            Ok(json!({
                "candidate_index": index,
                "candidate_local_id": string_field(projected, "rule_id")?,
                "created_order": integer_field(projected, "created_order")?,
                "listener_alias": rule.listener_alias()
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    let android_profile_ids = candidate
        .workspace
        .android_network_profiles
        .iter()
        .enumerate()
        .map(|(index, _)| {
            string_field(
                &projected_workspace["android_network_profiles"][index],
                "id",
            )
        })
        .collect::<AppResult<Vec<_>>>()?;

    let mut certificate_aliases = candidate.materials.certificate_aliases();
    certificate_aliases.sort_unstable();
    let mut secret_aliases = candidate.materials.secret_aliases();
    secret_aliases.sort_unstable();
    let snapshot = json!({
        "target_key": exact_public_target_key(&candidate.lifecycle_target()),
        "target": public_target(&candidate.target),
        "baseline_public": baseline_public(&candidate.target),
        "validation_layers": validation_layers,
        "resources": {
            "listeners": listeners,
            "http_rules": http_rules,
            "protocol_rules": protocol_rules,
            "android_profile_ids": android_profile_ids
        },
        "alias_graph": {
            "certificate_aliases": certificate_aliases,
            "secret_aliases": secret_aliases
        },
        "materials_public": {
            "certificates": candidate.materials.public_certificates(),
            "secrets": candidate.materials.public_secrets()
        },
        "protocol_document_values": protocol_document_value_contract(),
        "terminal_action_fields": {
            "TruncateResponse": ["bytes"],
            "DisconnectDuringUpstreamWrite": ["after_bytes"],
            "DisconnectDuringDownstreamWrite": ["after_bytes"]
        }
    });
    EnvironmentCandidatePublicSnapshot::from_validated_json(
        &serde_json::to_vec(&snapshot).map_err(|_| preview_failure())?,
    )
    .map_err(|_| preview_failure())
}

fn validation_layers_with_preview(
    prior_layers: &[EnvironmentValidationResult],
) -> AppResult<Vec<Value>> {
    let mut layers = serde_json::to_value(prior_layers)
        .map_err(|_| preview_failure())?
        .as_array()
        .cloned()
        .ok_or_else(preview_failure)?;
    layers.push(json!({
        "layer": "preview_baseline",
        "status": "passed",
        "code": null,
        "reason": null,
        "duration_ms": 0
    }));
    Ok(layers)
}

fn string_field(value: &Value, field: &str) -> AppResult<String> {
    value[field]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(preview_failure)
}

fn integer_field(value: &Value, field: &str) -> AppResult<u64> {
    value[field].as_u64().ok_or_else(preview_failure)
}

fn public_target(target: &EnvironmentTarget) -> Value {
    match target {
        EnvironmentTarget::Existing {
            workspace_id,
            expected_revision,
        } => json!({
            "mode": "existing",
            "workspace_id": workspace_id,
            "expected_revision": expected_revision,
        }),
        EnvironmentTarget::New { name } => json!({ "mode": "new", "name": name }),
    }
}

fn baseline_public(target: &EnvironmentTarget) -> Value {
    match target {
        EnvironmentTarget::Existing {
            workspace_id,
            expected_revision,
        } => json!({
            "workspace_id": workspace_id,
            "revision": expected_revision,
            "selected": false
        }),
        EnvironmentTarget::New { .. } => {
            json!({ "workspace_id": null, "revision": null, "selected": false })
        }
    }
}

fn protocol_document_value_contract() -> Vec<Value> {
    vec![
        json!({ "type": "string", "value": "abc" }),
        json!({ "type": "int", "value": 7 }),
        json!({ "type": "bool", "value": true }),
        json!({ "type": "blob", "value": [0, 255] }),
    ]
}

fn preview_failure() -> AppError {
    AppError::new(
        "VALIDATION_LAYER_FAILED",
        "environment preview validation failed",
    )
}
