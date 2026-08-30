use super::*;
use intercept_proxy_domain::RuleId as DomainRuleId;

use crate::environment_configuration::EnvironmentProjectedCandidate;
use crate::requirements_tests::{
    FakePorts, application_with_environment_preview_ports, http_rule_definitions,
    protocol_rule_definitions, test_environment_identity_allocator,
};
use crate::{
    EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentApplyGenerations, EnvironmentCandidateEpoch, EnvironmentValidatedApplyBaseline,
    InMemoryWorkspaceStore, ProxyWorkspace, WorkspaceId, WorkspaceRepositoryPort,
};

const UNKNOWN_ID: &str = "ffffffff-ffff-ffff-ffff-ffffffffffff";

#[derive(Default)]
struct CapturingBaseline {
    workspaces: Mutex<Vec<ProxyWorkspace>>,
}

#[async_trait::async_trait]
impl EnvironmentApplyBaselineCapturePort for CapturingBaseline {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> AppResult<EnvironmentValidatedApplyBaseline> {
        self.workspaces
            .lock()
            .unwrap()
            .push(request.candidate_workspace);
        Ok(EnvironmentValidatedApplyBaseline::validated(
            EnvironmentApplyGenerations::default(),
            [1; 32],
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ))
    }
}

async fn persisted_full_shape(
    store: &InMemoryWorkspaceStore,
) -> (ProxyWorkspace, serde_json::Value) {
    let candidate = crate::parse_environment_configuration_candidate_v1(FULL_SHAPE).unwrap();
    let allocator = test_environment_identity_allocator();
    let projected =
        EnvironmentProjectedCandidate::project(candidate, None, allocator.port()).unwrap();
    let persisted = store
        .import_workspace(projected.workspace().clone())
        .await
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    (persisted, value)
}

async fn persisted_joint_http_shape(
    store: &InMemoryWorkspaceStore,
) -> (ProxyWorkspace, serde_json::Value) {
    let mut value: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    value["workspace"]["rules"][0]["document"] = serde_json::json!({
        "package": {"id": "au-eftex", "version": "1.1.0"},
        "conditions": [{
            "operator": "equals",
            "field": "/amount",
            "value": 1000
        }],
        "actions": [{"type": "record_match"}]
    });
    let typed =
        crate::parse_environment_configuration_candidate_v1(&serde_json::to_vec(&value).unwrap())
            .unwrap();
    let allocator = test_environment_identity_allocator();
    let projected = EnvironmentProjectedCandidate::project(typed, None, allocator.port()).unwrap();
    let persisted = store
        .import_workspace(projected.workspace().clone())
        .await
        .unwrap();
    value["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    (persisted, value)
}

fn retain_all_ids(candidate: &mut serde_json::Value, persisted: &ProxyWorkspace) {
    for (listener, existing) in candidate["workspace"]["listeners"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&persisted.listeners)
    {
        listener["id"] = serde_json::json!(existing.id);
    }
    let http_rules = http_rule_definitions(persisted);
    let protocol_rules = protocol_rule_definitions(persisted);
    candidate["workspace"]["rules"][0]["existing_rule_id"] =
        serde_json::json!(http_rules[0].rule_id());
    candidate["workspace"]["rules"][14]["existing_rule_id"] =
        serde_json::json!(protocol_rules[0].rule_id());
}

async fn validate_existing(
    store: Arc<InMemoryWorkspaceStore>,
    capture: Arc<CapturingBaseline>,
    candidate: serde_json::Value,
) -> crate::EnvironmentValidationReport {
    let bytes = serde_json::to_vec(&candidate).unwrap();
    let typed = crate::parse_environment_configuration_candidate_v1(&bytes).unwrap();
    let application = application_with_environment_preview_ports(
        Arc::new(FakePorts::default()),
        store,
        capture,
        test_environment_identity_allocator(),
    );
    let inserted = application
        .environment_candidate_insert_validating(typed, EnvironmentCandidateEpoch::new(1))
        .unwrap();
    let cancellation = CancellationToken::new();
    validator(Arc::new(RecordingValidationPort::new(Behavior::Pass)))
        .validate_for_candidate(
            inserted.candidate_id(),
            &bytes,
            cancellation.clone(),
            cancellation,
            &application,
        )
        .await
}

async fn assert_existing_domain_code(
    store: Arc<InMemoryWorkspaceStore>,
    candidate: serde_json::Value,
    expected: EnvironmentStatusCode,
) {
    let report = validate_existing(store, Arc::new(CapturingBaseline::default()), candidate).await;
    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(report.layers()[1].code(), Some(expected));
    for layer in &report.layers()[2..] {
        assert_eq!(
            layer.status(),
            EnvironmentValidationStatus::SkippedDependency
        );
    }
}

#[tokio::test]
async fn existing_listener_id_outside_target_workspace_fails_domain() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (_, mut candidate) = persisted_full_shape(&store).await;
    candidate["workspace"]["listeners"][0]["id"] = serde_json::json!(UNKNOWN_ID);
    let report = validate_existing(store, Arc::new(CapturingBaseline::default()), candidate).await;

    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[1].code(),
        Some(EnvironmentStatusCode::ListenerDomainInvalid)
    );
}

#[tokio::test]
async fn unknown_existing_http_rule_id_fails_with_exact_code() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (_, mut candidate) = persisted_full_shape(&store).await;
    candidate["workspace"]["rules"][0]["existing_rule_id"] = serde_json::json!(UNKNOWN_ID);
    let report = validate_existing(store, Arc::new(CapturingBaseline::default()), candidate).await;

    assert_eq!(
        report.layers()[1].status(),
        EnvironmentValidationStatus::Failed
    );
    assert_eq!(
        report.layers()[1].code(),
        Some(EnvironmentStatusCode::ExistingRuleIdUnknown)
    );
}

#[tokio::test]
async fn cross_workspace_existing_http_rule_id_fails_with_exact_code() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    let mut other = persisted.clone();
    other.id = WorkspaceId::new();
    other.name = "Other Workspace".into();
    let existing_http_id = http_rule_definitions(&other)[0].rule_id();
    let index = other
        .rule_definitions
        .iter()
        .position(|rule| rule.rule_id() == existing_http_id)
        .unwrap();
    let existing = other.rule_definitions[index].clone();
    other.rule_definitions[index] = intercept_proxy_domain::RuleDefinition::restore(
        DomainRuleId::from_uuid(uuid::Uuid::new_v4()),
        existing.to_draft(),
        intercept_proxy_domain::RuleDefinitionRestoreSnapshot {
            revision: existing.revision(),
            created_order: existing.created_order(),
            lifecycle: existing.lifecycle().clone(),
        },
    )
    .unwrap();
    let other = store.import_workspace(other).await.unwrap();
    candidate["workspace"]["rules"][0]["existing_rule_id"] =
        serde_json::json!(http_rule_definitions(&other)[0].rule_id());

    assert_existing_domain_code(
        store,
        candidate,
        EnvironmentStatusCode::ExistingRuleIdWorkspaceMismatch,
    )
    .await;
}

#[tokio::test]
async fn existing_rule_id_of_the_wrong_kind_fails_with_exact_code() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    candidate["workspace"]["rules"][0]["existing_rule_id"] =
        serde_json::json!(protocol_rule_definitions(&persisted)[0].rule_id());

    assert_existing_domain_code(
        store,
        candidate,
        EnvironmentStatusCode::ExistingRuleIdKindMismatch,
    )
    .await;
}

#[tokio::test]
async fn existing_http_rule_listener_binding_mismatch_fails_with_exact_code() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let mut candidate: serde_json::Value = serde_json::from_slice(FULL_SHAPE).unwrap();
    let mut alternate = candidate["workspace"]["listeners"][0].clone();
    alternate["alias"] = serde_json::json!("http-entry-alt");
    alternate["name"] = serde_json::json!("HTTP entry alternate");
    alternate["port"] = serde_json::json!(8081);
    let settings = &mut alternate["data_plane"]["settings"];
    settings["authentication"] = serde_json::json!({"mode": "none"});
    settings["mitm"] = serde_json::json!({
        "enabled": false,
        "authority_allowlist": [],
        "root_ca_selector": null,
        "maximum_cached_leaf_certificates": 0,
    });
    settings["downstream_tls"] = serde_json::json!({
        "enabled": false,
        "server_identity_alias": null,
        "dynamic_sni_allowlist": [],
        "client_authentication": {"mode": "disabled"},
    });
    settings["body_processing"] = serde_json::json!({"mode": "plain"});
    settings["fixed_server"] = serde_json::json!({
        "upstream_url": "http://pay.example.test",
        "upstream_tls": {
            "verify_hostname": false,
            "server_trust_alias": null,
            "client_identity_alias": null,
        },
    });
    candidate["workspace"]["listeners"]
        .as_array_mut()
        .unwrap()
        .push(alternate);
    let typed = crate::parse_environment_configuration_candidate_v1(
        &serde_json::to_vec(&candidate).unwrap(),
    )
    .unwrap();
    let allocator = test_environment_identity_allocator();
    let projected = EnvironmentProjectedCandidate::project(typed, None, allocator.port()).unwrap();
    let persisted = store
        .import_workspace(projected.workspace().clone())
        .await
        .unwrap();
    candidate["target"] = serde_json::json!({
        "mode": "existing",
        "workspace_id": persisted.id,
        "expected_revision": persisted.revision,
    });
    retain_all_ids(&mut candidate, &persisted);
    candidate["workspace"]["listeners"][2]["id"] = serde_json::json!(persisted.listeners[2].id);
    candidate["workspace"]["rules"][0]["listener_alias"] = serde_json::json!("http-entry-alt");

    assert_existing_domain_code(
        store,
        candidate,
        EnvironmentStatusCode::ExistingRuleIdBindingMismatch,
    )
    .await;
}

#[tokio::test]
async fn existing_protocol_rule_package_mismatch_fails_with_exact_code() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    candidate["workspace"]["rules"][14]["package"]["version"] = serde_json::json!("1.2.0");

    assert_existing_domain_code(
        store,
        candidate,
        EnvironmentStatusCode::ExistingRuleIdPackageMismatch,
    )
    .await;
}

#[tokio::test]
async fn existing_protocol_rule_can_change_stage_without_changing_binding_or_content() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    let original = protocol_rule_definitions(&persisted)[0];
    let original_id = original.rule_id();
    let original_created_order = original.created_order();
    let original_listener_id = original.listener_id();
    let original_content = original.content().clone();
    candidate["workspace"]["rules"][14]["stage"] = serde_json::json!("proxy_to_app");
    let capture = Arc::new(CapturingBaseline::default());
    let report = validate_existing(store, capture.clone(), candidate).await;

    assert_eq!(report.status_code(), None);
    let captured = capture.workspaces.lock().unwrap();
    let updated = captured[0]
        .rule_definitions
        .iter()
        .find(|rule| rule.rule_id() == original_id)
        .unwrap();
    assert_eq!(updated.created_order(), original_created_order);
    assert_eq!(updated.listener_id(), original_listener_id);
    assert_eq!(updated.content(), &original_content);
    assert_eq!(
        updated.stage(),
        intercept_proxy_domain::RuleStage::ProxyToApp
    );
}

#[tokio::test]
async fn existing_protocol_rule_stage_change_still_rejects_invalid_listener_topology() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    candidate["workspace"]["listeners"][1]["data_plane"]["settings"]["topology"] = serde_json::json!({
        "mode": "local_responder",
        "settings": {"downstream_security": {"mode": "tcp"}}
    });
    candidate["materials"]["certificates"]
        .as_array_mut()
        .unwrap()
        .retain(|material| {
            !matches!(
                material["alias"].as_str(),
                Some("socket-listener-identity" | "socket-upstream-client")
            )
        });
    candidate["workspace"]["rules"][14]["stage"] = serde_json::json!("app_to_proxy");

    assert_existing_domain_code(
        store,
        candidate,
        EnvironmentStatusCode::ProtocolDocumentRuleInvalid,
    )
    .await;
}

#[tokio::test]
async fn existing_plain_http_rule_can_change_stage_without_changing_identity() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    candidate["workspace"]["rules"][0]["stage"] = serde_json::json!("proxy_to_upstream");
    let capture = Arc::new(CapturingBaseline::default());
    let report = validate_existing(store, capture.clone(), candidate).await;

    assert_eq!(report.status_code(), None);
    let captured = capture.workspaces.lock().unwrap();
    let original_id = http_rule_definitions(&persisted)[0].rule_id();
    let updated = captured[0]
        .rule_definitions
        .iter()
        .find(|rule| rule.rule_id() == original_id)
        .unwrap();
    assert_eq!(
        updated.stage(),
        intercept_proxy_domain::RuleStage::ProxyToUpstream
    );
}

#[tokio::test]
async fn existing_joint_http_rule_updates_editable_fields_and_keeps_immutable_binding() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_joint_http_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    let original = persisted
        .rule_definitions
        .iter()
        .find(|rule| {
            matches!(
                rule.content(),
                intercept_proxy_domain::RuleContent::Http(content) if content.document.is_some()
            )
        })
        .unwrap();
    candidate["workspace"]["rules"][0]["existing_rule_id"] = serde_json::json!(original.rule_id());
    let original_socket = persisted
        .rule_definitions
        .iter()
        .find(|rule| {
            matches!(
                rule.content(),
                intercept_proxy_domain::RuleContent::Socket(_)
            )
        })
        .unwrap();
    candidate["workspace"]["rules"][14]["existing_rule_id"] =
        serde_json::json!(original_socket.rule_id());
    candidate["workspace"]["rules"][0]["name"] = serde_json::json!("Joint HTTP updated");
    candidate["workspace"]["rules"][0]["stage"] = serde_json::json!("proxy_to_upstream");
    candidate["workspace"]["rules"][0]["actions"] =
        serde_json::json!([{"Delay": {"milliseconds": 7}}]);
    let capture = Arc::new(CapturingBaseline::default());
    let report = validate_existing(store, capture.clone(), candidate).await;

    assert_eq!(report.status_code(), None);
    let captured = capture.workspaces.lock().unwrap();
    let updated = captured[0]
        .rule_definitions
        .iter()
        .find(|rule| rule.rule_id() == original.rule_id())
        .unwrap();
    assert_eq!(updated.name(), "Joint HTTP updated");
    assert_eq!(
        updated.stage(),
        intercept_proxy_domain::RuleStage::ProxyToUpstream
    );
    assert_eq!(updated.listener_id(), original.listener_id());
    assert_eq!(updated.created_order(), original.created_order());
    let intercept_proxy_domain::RuleContent::Http(content) = updated.content() else {
        panic!("joint HTTP rule must remain HTTP content");
    };
    assert!(content.document.is_some());
}

#[tokio::test]
async fn valid_existing_http_and_protocol_rules_retain_id_and_created_order() {
    let store = Arc::new(InMemoryWorkspaceStore::new_empty());
    let (persisted, mut candidate) = persisted_full_shape(&store).await;
    retain_all_ids(&mut candidate, &persisted);
    let capture = Arc::new(CapturingBaseline::default());
    let report = validate_existing(store, capture.clone(), candidate).await;

    assert_eq!(report.status_code(), None);
    let captured = capture.workspaces.lock().unwrap();
    let captured_http = http_rule_definitions(&captured[0]);
    let persisted_http = http_rule_definitions(&persisted);
    let captured_protocol = protocol_rule_definitions(&captured[0]);
    let persisted_protocol = protocol_rule_definitions(&persisted);
    assert_eq!(captured_http[0].rule_id(), persisted_http[0].rule_id());
    assert_eq!(
        captured_http[0].created_order(),
        persisted_http[0].created_order()
    );
    assert_eq!(
        captured_protocol[0].rule_id(),
        persisted_protocol[0].rule_id()
    );
    assert_eq!(
        captured_protocol[0].created_order(),
        persisted_protocol[0].created_order()
    );
}
