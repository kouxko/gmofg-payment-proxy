use super::*;

fn protocol_document() -> serde_json::Value {
    serde_json::json!({
        "package": {"id": "au-eftex", "version": "1.1.0"},
        "conditions": [{"operator": "equals", "field": "/amount", "value": 1000}],
        "actions": [{"type": "record_match"}]
    })
}

#[tokio::test]
async fn application_boundary_stages_reject_ordinary_http_conditions_and_actions() {
    for stage in ["app_to_proxy", "upstream_to_proxy"] {
        let candidate = domain_contract_red::candidate_json_with(|candidate| {
            candidate["workspace"]["rules"][0]["stage"] = serde_json::json!(stage);
        });

        domain_contract_red::assert_domain_code_before_preview(
            &candidate,
            EnvironmentStatusCode::HttpRuleInvalid,
        )
        .await;
    }
}

#[tokio::test]
async fn environment_domain_accepts_pure_document_and_exact_joint_stages() {
    for (stage, pure_document) in [
        ("proxy_to_upstream", true),
        ("proxy_to_app", true),
        ("proxy_to_upstream", false),
        ("proxy_to_app", false),
    ] {
        let candidate = domain_contract_red::candidate_json_with(|candidate| {
            let rule = &mut candidate["workspace"]["rules"][0];
            rule["stage"] = serde_json::json!(stage);
            rule["document"] = protocol_document();
            if pure_document {
                rule["conditions"] = serde_json::json!([]);
                rule["actions"] = serde_json::json!([]);
            } else {
                rule["conditions"] = serde_json::json!([]);
                rule["actions"] = serde_json::json!([{
                    "Delay": {"milliseconds": 1}
                }]);
            }
        });
        let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
            .validate(&candidate, CancellationToken::new())
            .await;

        assert_eq!(
            report.layers()[1].status(),
            EnvironmentValidationStatus::Passed,
            "stage={stage}"
        );
    }
}

#[tokio::test]
async fn request_stage_custom_status_fails_with_exact_http_rule_code() {
    let candidate = domain_contract_red::candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"CustomHttpStatus": {"status": 503}}));
    });

    domain_contract_red::assert_domain_code_before_preview(
        &candidate,
        EnvironmentStatusCode::HttpRuleInvalid,
    )
    .await;
}

#[tokio::test]
async fn request_stage_downstream_throttle_fails_with_exact_http_rule_code() {
    let candidate = domain_contract_red::candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "Throttle": {
                    "bytes_per_second": 1024,
                    "chunk_bytes": 128,
                    "direction": "Downstream"
                }
            }));
    });

    domain_contract_red::assert_domain_code_before_preview(
        &candidate,
        EnvironmentStatusCode::HttpRuleInvalid,
    )
    .await;
}

#[tokio::test]
async fn two_terminal_actions_fail_with_exact_http_rule_code() {
    let candidate = domain_contract_red::candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][3]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "Terminal": {"UpstreamConnectTimeout": {"milliseconds": 1}}
            }));
    });

    domain_contract_red::assert_domain_code_before_preview(
        &candidate,
        EnvironmentStatusCode::HttpRuleInvalid,
    )
    .await;
}

#[tokio::test]
async fn non_terminal_action_after_terminal_fails_with_exact_http_rule_code() {
    let candidate = domain_contract_red::candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][3]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!("Pause"));
    });

    domain_contract_red::assert_domain_code_before_preview(
        &candidate,
        EnvironmentStatusCode::HttpRuleInvalid,
    )
    .await;
}
