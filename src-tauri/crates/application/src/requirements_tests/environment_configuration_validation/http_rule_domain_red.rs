use super::*;

fn protocol_document() -> serde_json::Value {
    serde_json::json!({
        "package": {"id": "au-eftex", "version": "1.1.0"},
    })
}

fn document_condition() -> serde_json::Value {
    serde_json::json!({
        "operator": "leaf",
        "children": {
            "source": "document",
            "path": "/amount",
            "predicate": {"type":"number","value":{"operator":"equal","value":1000}}
        }
    })
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
            rule["content"]["value"]["document"] = protocol_document();
            rule["content"]["value"]["condition"] = document_condition();
            if pure_document {
                rule["content"]["value"]["actions"] =
                    serde_json::json!([{"source":"record_match"}]);
            } else {
                rule["content"]["value"]["actions"] = serde_json::json!([{
                    "source":"http", "value":{"Delay": {"milliseconds": 1}}
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
        candidate["workspace"]["rules"][0]["content"]["value"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(
                serde_json::json!({"source":"http","value":{"CustomHttpStatus": {"status": 503}}}),
            );
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
        candidate["workspace"]["rules"][0]["content"]["value"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"source":"http","value":{
                "Throttle": {
                    "bytes_per_second": 1024,
                    "chunk_bytes": 128,
                    "direction": "Downstream"
                }}
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
        candidate["workspace"]["rules"][3]["content"]["value"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "source":"terminal", "value":{"UpstreamConnectTimeout": {"milliseconds": 1}}
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
        candidate["workspace"]["rules"][3]["content"]["value"]["actions"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"source":"http","value":"Pause"}));
    });

    domain_contract_red::assert_domain_code_before_preview(
        &candidate,
        EnvironmentStatusCode::HttpRuleInvalid,
    )
    .await;
}
