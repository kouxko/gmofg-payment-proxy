use super::*;

fn candidate_json_with(mut edit: impl FnMut(&mut serde_json::Value)) -> Vec<u8> {
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    edit(&mut candidate);
    serde_json::to_vec(&candidate).unwrap()
}

async fn assert_schema_code(candidate: &[u8], expected: EnvironmentStatusCode) {
    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate(candidate, CancellationToken::new())
        .await;

    assert_eq!(
        report.layers()[0].layer(),
        EnvironmentValidationLayer::Schema
    );
    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[0].code(), Some(expected));
    assert_eq!(report.status_code(), Some(expected));
}

#[tokio::test]
async fn existing_rule_id_on_new_target_preserves_forbidden_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["http_rules"][0]["existing_rule_id"] =
            serde_json::json!("00000000-0000-0000-0000-000000000020");
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::ExistingRuleIdForbidden).await;
}

#[tokio::test]
async fn duplicate_existing_rule_id_preserves_duplicate_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["target"] = serde_json::json!({
            "mode": "existing",
            "workspace_id": "00000000-0000-0000-0000-000000000001",
            "expected_revision": 7,
        });
        let rules = candidate["workspace"]["http_rules"].as_array_mut().unwrap();
        rules[0]["existing_rule_id"] = serde_json::json!("00000000-0000-0000-0000-000000000020");
        let mut duplicate = rules[0].clone();
        duplicate["name"] = serde_json::json!("Duplicate selector");
        rules.push(duplicate);
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::ExistingRuleIdDuplicate).await;
}

#[tokio::test]
async fn invalid_weak_network_value_preserves_value_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["android_network_profiles"][0]["weak_network"]["random_loss_basis_points"] =
            serde_json::json!(10_001);
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::WeakNetworkValueInvalid).await;
}

#[tokio::test]
async fn unknown_root_field_preserves_unknown_field_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["unexpected"] = serde_json::json!(true);
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::UnknownField).await;
}

#[tokio::test]
async fn unknown_nested_field_preserves_unknown_field_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][0]["unexpected"] = serde_json::json!(true);
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::UnknownField).await;
}

#[tokio::test]
async fn client_submitted_validation_request_preserves_forbidden_field_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["validation_request"] = serde_json::json!({});
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::ForbiddenField).await;
}

#[tokio::test]
async fn unsupported_mitm_root_material_preserves_material_role_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["materials"]["certificates"][0]["role"] = serde_json::json!("mitm_root_ca");
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::UnsupportedMaterialRole).await;
}

#[tokio::test]
async fn invalid_document_value_wire_preserves_document_wire_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["protocol_rules"][0]["conditions"][0]["value"] =
            serde_json::json!("raw-scalar-is-forbidden");
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::DocumentValueWireInvalid).await;
}

#[tokio::test]
async fn invalid_weak_network_wire_preserves_weak_network_wire_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["android_network_profiles"][0]["weak_network"]["burst_loss"] =
            serde_json::json!(100);
    });

    assert_schema_code(&candidate, EnvironmentStatusCode::WeakNetworkWireInvalid).await;
}
