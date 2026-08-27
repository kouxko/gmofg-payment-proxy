use super::*;

#[tokio::test]
async fn request_stage_custom_status_fails_with_exact_http_rule_code() {
    let candidate = domain_contract_red::candidate_json_with(|candidate| {
        candidate["workspace"]["http_rules"][0]["actions"]
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
        candidate["workspace"]["http_rules"][0]["actions"]
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
        candidate["workspace"]["http_rules"][3]["actions"]
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
        candidate["workspace"]["http_rules"][3]["actions"]
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
