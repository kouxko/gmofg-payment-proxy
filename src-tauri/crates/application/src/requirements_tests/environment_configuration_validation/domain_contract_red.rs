use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::environment_configuration::{
    EnvironmentPreviewBaselinePort, EnvironmentPreviewBaselineRequest,
};

#[derive(Default)]
struct CountingPreviewPort {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl EnvironmentPreviewBaselinePort for CountingPreviewPort {
    async fn validate_preview_baseline(
        &self,
        _: EnvironmentPreviewBaselineRequest<'_>,
    ) -> AppResult<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

pub(super) fn candidate_json_with(mut edit: impl FnMut(&mut serde_json::Value)) -> Vec<u8> {
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    edit(&mut candidate);
    serde_json::to_vec(&candidate).unwrap()
}

async fn assert_domain_code(candidate: &[u8], expected: EnvironmentStatusCode) {
    let report = validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate(candidate, CancellationToken::new())
        .await;
    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[1].layer(),
        EnvironmentValidationLayer::Domain
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[1].code(), Some(expected));
    assert_eq!(report.status_code(), Some(expected));
}

pub(super) async fn assert_domain_code_before_preview(
    candidate: &[u8],
    expected: EnvironmentStatusCode,
) {
    let (_registry, candidate_id, candidate_cancellation) =
        review_red::validating_registry_candidate();
    let validation_port = Arc::new(RecordingValidationPort::new(Behavior::Pass));
    let preview_port = CountingPreviewPort::default();
    let report = validator(Arc::clone(&validation_port))
        .validate_for_candidate(
            &candidate_id,
            candidate,
            CancellationToken::new(),
            candidate_cancellation,
            &preview_port,
        )
        .await;

    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[1].layer(),
        EnvironmentValidationLayer::Domain
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[1].code(), Some(expected));
    assert_eq!(report.status_code(), Some(expected));
    assert_eq!(
        validation_port.calls(),
        vec![EnvironmentValidationLayer::Schema]
    );
    assert_eq!(preview_port.calls.load(Ordering::SeqCst), 0);
    for layer in &report.layers()[2..] {
        assert_eq!(
            layer.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
        assert_eq!(layer.reason(), Some("dependency_not_satisfied"));
    }
}

#[tokio::test]
async fn duplicate_enabled_listener_endpoint_fails_domain_before_preview() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][1]["bind_address"] = serde_json::json!("0.0.0.0");
        candidate["workspace"]["listeners"][1]["port"] = serde_json::json!(8080);
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::ListenerDomainInvalid)
        .await;
}

#[tokio::test]
async fn http_rule_without_action_fails_schema_before_preview() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["content"]["value"]
            .as_object_mut()
            .unwrap()
            .remove("action");
    });

    selector_parse_red::assert_schema_code(&candidate, EnvironmentStatusCode::SchemaInvalid).await;
}

#[tokio::test]
async fn http_method_with_non_equals_operator_fails_domain_with_exact_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["content"]["value"]["condition"]["field"] =
            serde_json::json!("Method");
        candidate["workspace"]["rules"][0]["content"]["value"]["condition"]["operator"] =
            serde_json::json!({"Contains": "PO"});
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::HttpRuleInvalid).await;
}

#[tokio::test]
async fn http_rule_invalid_action_value_fails_domain_with_exact_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["content"]["value"]["action"] = serde_json::json!({
            "source": "http",
            "value": {"Delay": {"milliseconds": 0}}
        });
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::HttpRuleInvalid).await;
}

#[tokio::test]
async fn http_rule_invalid_rate_fails_domain_with_exact_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["content"]["value"]["action"] = serde_json::json!({
            "source": "http",
            "value": {"Throttle": {
                "bytes_per_second": 0,
                "chunk_bytes": 1,
                "direction": "Upstream"
            }}
        });
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::HttpRuleInvalid).await;
}

#[tokio::test]
async fn http_rule_invalid_timeout_fails_domain_with_exact_code() {
    let candidate = candidate_json_with(|candidate| {
        rule_named_mut(candidate, "Upstream connect timeout")["content"]["value"]["action"]["value"]
            ["UpstreamConnectTimeout"]["milliseconds"] = serde_json::json!(0);
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::HttpRuleInvalid).await;
}

#[tokio::test]
async fn http_rule_bound_to_socket_listener_fails_with_alias_type_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["rules"][0]["listener_alias"] = serde_json::json!("socket-entry");
    });

    assert_domain_code_before_preview(&candidate, EnvironmentStatusCode::ListenerAliasTypeMismatch)
        .await;
}

#[tokio::test(start_paused = true)]
async fn schema_work_is_cancelled_by_its_one_second_layer_budget() {
    let report =
        EnvironmentCandidateValidator::new(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
            .with_layer_budget(EnvironmentValidationLayer::Schema, Duration::ZERO)
            .validate(b"{not-json", CancellationToken::new())
            .await;

    assert_eq!(
        report.layers()[0].layer(),
        EnvironmentValidationLayer::Schema
    );
    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[0].reason(), Some("layer_budget_exceeded"));
}

#[tokio::test(start_paused = true)]
async fn domain_work_is_cancelled_by_its_one_second_layer_budget() {
    let candidate = candidate_json_with(|candidate| {
        candidate["target"]["name"] = serde_json::json!("   ");
    });
    let report =
        EnvironmentCandidateValidator::new(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
            .with_layer_budget(EnvironmentValidationLayer::Domain, Duration::ZERO)
            .validate(&candidate, CancellationToken::new())
            .await;

    assert_eq!(
        report.layers()[0].status(),
        EnvironmentValidationStatus::Passed
    );
    assert_eq!(
        report.layers()[1].layer(),
        EnvironmentValidationLayer::Domain
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[1].reason(), Some("layer_budget_exceeded"));
}

#[tokio::test]
async fn candidate_cancellation_interrupts_the_domain_layer_operation() {
    let (registry, candidate_id, cancellation) = review_red::validating_registry_candidate();
    let port = Arc::new(RecordingValidationPort::new(Behavior::Block(
        EnvironmentValidationLayer::Domain,
    )));
    let task = tokio::spawn({
        let port = Arc::clone(&port);
        async move { validator(port).validate(FULL_SHAPE, cancellation).await }
    });
    while port.calls().last() != Some(&EnvironmentValidationLayer::Domain) {
        tokio::task::yield_now().await;
    }

    registry.cancel(&candidate_id);
    let report = tokio::time::timeout(Duration::from_millis(100), task)
        .await
        .expect("candidate cancellation interrupts domain")
        .unwrap();

    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::CandidateCancelled)
    );
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Cancelled
    );
}

fn extend_with_unique_clones(array: &mut Vec<serde_json::Value>, limit: usize, key: &str) {
    let template = array[0].clone();
    while array.len() <= limit {
        let mut item = template.clone();
        item[key] = serde_json::json!(format!("limit-case-{}", array.len()));
        array.push(item);
    }
}

#[tokio::test]
async fn rejects_more_than_eight_listeners() {
    let candidate = candidate_json_with(|candidate| {
        let listeners = candidate["workspace"]["listeners"].as_array_mut().unwrap();
        let template = listeners[2].clone();
        while listeners.len() <= 8 {
            let mut listener = template.clone();
            listener["alias"] = serde_json::json!(format!("listener-{}", listeners.len()));
            listener["name"] = serde_json::json!(format!("Listener {}", listeners.len()));
            listener["port"] = serde_json::json!(10_000 + listeners.len());
            listeners.push(listener);
        }
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_one_hundred_twenty_eight_http_rules() {
    let candidate = candidate_json_with(|candidate| {
        extend_with_unique_clones(
            candidate["workspace"]["rules"].as_array_mut().unwrap(),
            128,
            "name",
        );
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_one_hundred_twenty_eight_protocol_rules() {
    let candidate = candidate_json_with(|candidate| {
        extend_with_unique_clones(
            candidate["workspace"]["rules"].as_array_mut().unwrap(),
            128,
            "name",
        );
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_sixteen_certificates_before_alias_checks() {
    let candidate = candidate_json_with(|candidate| {
        extend_with_unique_clones(
            candidate["materials"]["certificates"]
                .as_array_mut()
                .unwrap(),
            16,
            "alias",
        );
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_sixteen_secrets_before_alias_checks() {
    let candidate = candidate_json_with(|candidate| {
        extend_with_unique_clones(
            candidate["materials"]["secrets"].as_array_mut().unwrap(),
            16,
            "alias",
        );
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_more_than_eight_android_profiles() {
    let candidate = candidate_json_with(|candidate| {
        extend_with_unique_clones(
            candidate["workspace"]["android_network_profiles"]
                .as_array_mut()
                .unwrap(),
            8,
            "name",
        );
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::DtoLimitExceeded).await;
}

#[tokio::test]
async fn rejects_a_missing_material_alias_with_its_registered_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][0]["data_plane"]["settings"]["downstream_tls"]["server_identity_alias"] =
            serde_json::json!("missing-identity");
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::MaterialAliasMissing).await;
}

#[tokio::test]
async fn rejects_a_material_alias_role_mismatch_with_its_registered_code() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][0]["data_plane"]["settings"]["downstream_tls"]["server_identity_alias"] =
            serde_json::json!("downstream-trust");
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::MaterialAliasTypeMismatch).await;
}

#[tokio::test]
async fn rejects_a_submitted_material_with_zero_consumers() {
    let candidate = candidate_json_with(|candidate| {
        let certificates = candidate["materials"]["certificates"]
            .as_array_mut()
            .unwrap();
        let mut unused = certificates[0].clone();
        unused["alias"] = serde_json::json!("unused-certificate");
        certificates.push(unused);
    });
    assert_domain_code(&candidate, EnvironmentStatusCode::MaterialAliasUnused).await;
}

#[tokio::test]
async fn rejects_identity_alias_reuse_by_multiple_consumers() {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["topology"]["settings"]
            ["security"]["upstream_tls"]["client_identity_alias"] =
            serde_json::json!("http-upstream-client");
        candidate["materials"]["certificates"]
            .as_array_mut()
            .unwrap()
            .retain(|material| material["alias"] != "socket-upstream-client");
    });
    assert_domain_code(
        &candidate,
        EnvironmentStatusCode::MaterialAliasMultipleConsumersUnsupported,
    )
    .await;
}

#[tokio::test]
async fn rejects_credential_alias_reuse_by_multiple_consumers() {
    let candidate = candidate_json_with(|candidate| {
        let mut listener = candidate["workspace"]["listeners"][0].clone();
        listener["alias"] = serde_json::json!("second-http-entry");
        listener["name"] = serde_json::json!("Second HTTP entry");
        listener["port"] = serde_json::json!(8081);
        listener["data_plane"]["settings"]["downstream_tls"]["server_identity_alias"] =
            serde_json::json!("second-http-listener-identity");
        listener["data_plane"]["settings"]["fixed_server"]["upstream_tls"]["client_identity_alias"] =
            serde_json::json!("second-http-upstream-client");
        candidate["workspace"]["listeners"]
            .as_array_mut()
            .unwrap()
            .push(listener);

        let certificates = candidate["materials"]["certificates"]
            .as_array_mut()
            .unwrap();
        for (source, alias) in [
            ("http-listener-identity", "second-http-listener-identity"),
            ("http-upstream-client", "second-http-upstream-client"),
        ] {
            let mut copy = certificates
                .iter()
                .find(|material| material["alias"] == source)
                .unwrap()
                .clone();
            copy["alias"] = serde_json::json!(alias);
            certificates.push(copy);
        }
    });
    assert_domain_code(
        &candidate,
        EnvironmentStatusCode::MaterialAliasMultipleConsumersUnsupported,
    )
    .await;
}

async fn assert_fixed_server_origin_rejected(upstream_url: &str) {
    let candidate = candidate_json_with(|candidate| {
        candidate["workspace"]["listeners"][0]["data_plane"]["settings"]["fixed_server"]["upstream_url"] =
            serde_json::json!(upstream_url);
    });
    let original = candidate.clone();
    let port = Arc::new(RecordingValidationPort::new(Behavior::Pass));

    let report = validator(Arc::clone(&port))
        .validate(&candidate, CancellationToken::new())
        .await;

    assert_eq!(candidate, original, "validation must not normalize input");
    assert_eq!(
        report.layers()[1].code(),
        Some(EnvironmentStatusCode::ListenerDomainInvalid)
    );
    assert_eq!(
        report.status_code(),
        Some(EnvironmentStatusCode::ListenerDomainInvalid)
    );
    assert_eq!(port.calls(), vec![EnvironmentValidationLayer::Schema]);
    for layer in &report.layers()[2..] {
        assert_eq!(
            layer.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
        assert_eq!(layer.reason(), Some("dependency_not_satisfied"));
    }
}

#[tokio::test]
async fn rejects_a_fixed_server_origin_with_a_path() {
    assert_fixed_server_origin_rejected("https://pay.example.test/transactions").await;
}

#[tokio::test]
async fn rejects_a_fixed_server_origin_with_a_query() {
    assert_fixed_server_origin_rejected("https://pay.example.test?merchant=1").await;
}

#[tokio::test]
async fn rejects_a_fixed_server_origin_with_a_fragment() {
    assert_fixed_server_origin_rejected("https://pay.example.test#checkout").await;
}

#[tokio::test]
async fn rejects_a_fixed_server_origin_with_userinfo() {
    assert_fixed_server_origin_rejected("https://operator@pay.example.test").await;
}
