use std::{collections::BTreeSet, sync::Mutex};

use async_trait::async_trait;
use intercept_proxy_domain::{
    AndroidNetworkProfile, AndroidProxyRoute, CertificateReference, CertificateReferenceId,
    CertificateReferenceKind, HttpBodyProcessing, ListenerDataPlane, MessageStage,
    ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion, Rule, RuleAction, RuleDraft,
    WeakNetworkProfile,
};

use crate::{
    AppError, EnvironmentApplyBaselineCapturePort, EnvironmentApplyBaselineCaptureRequest,
    EnvironmentCommitTarget, ProxyWorkspace,
};

fn existing_request(
    persisted_workspace: ProxyWorkspace,
    candidate_workspace: ProxyWorkspace,
) -> EnvironmentApplyBaselineCaptureRequest {
    EnvironmentApplyBaselineCaptureRequest {
        target: EnvironmentCommitTarget::Existing {
            workspace_id: persisted_workspace.id.as_uuid(),
            expected_revision: persisted_workspace.revision.get(),
        },
        persisted_workspace: Some(persisted_workspace),
        candidate_workspace,
        schema_version: 1,
        validation_engine_version: crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
    }
}

#[derive(Default)]
struct RecordingCapturePort {
    requests: Mutex<Vec<EnvironmentApplyBaselineCaptureRequest>>,
}

#[test]
fn unchanged_existing_listener_is_not_in_the_affected_runtime_scope() {
    let persisted = ProxyWorkspace::default();
    let request = existing_request(persisted.clone(), persisted);

    assert!(request.affected_listener_ids().is_empty());
}

#[test]
fn changed_existing_listener_is_lifted_into_the_affected_runtime_scope() {
    let persisted = ProxyWorkspace::default();
    let listener_id = persisted.listeners[0].id;
    let mut candidate = persisted.clone();
    candidate.listeners[0].read_timeout_ms += 1;
    let request = existing_request(persisted, candidate);

    assert_eq!(request.affected_listener_ids(), &[listener_id]);
}

#[test]
fn changed_http_rule_body_lifts_its_listener_into_the_affected_runtime_scope() {
    let mut persisted = ProxyWorkspace::default();
    let listener_id = persisted.listeners[0].id;
    persisted.rules.push(
        Rule::create(RuleDraft {
            expected_revision: None,
            name: "body rewrite".into(),
            description: String::new(),
            enabled: true,
            priority: 10,
            created_order: 1,
            channel: None,
            stage: MessageStage::Request,
            conditions: Vec::new(),
            actions: vec![RuleAction::ReplaceBodyText("before".into())],
            one_shot: false,
        })
        .expect("valid HTTP rule"),
    );
    let mut candidate = persisted.clone();
    candidate.rules[0].actions = vec![RuleAction::ReplaceBodyText("after".into())];

    assert_eq!(
        existing_request(persisted, candidate).affected_listener_ids(),
        &[listener_id]
    );
}

#[test]
fn material_only_change_lifts_every_listener_that_consumes_the_reference() {
    let mut persisted = ProxyWorkspace::default();
    let listener_id = persisted.listeners[0].id;
    let reference_id = CertificateReferenceId::new();
    let ListenerDataPlane::Http(settings) = &mut persisted.listeners[0].data_plane else {
        panic!("default Listener is HTTP");
    };
    settings.mitm.root_ca = Some(reference_id);
    persisted.certificate_references.push(CertificateReference {
        id: reference_id,
        label: "before".into(),
        kind: CertificateReferenceKind::MitmRootCa,
        reference: "managed:before".into(),
    });
    let mut candidate = persisted.clone();
    candidate.certificate_references[0].label = "after".into();
    candidate.certificate_references[0].reference = "managed:after".into();

    assert_eq!(
        existing_request(persisted, candidate).affected_listener_ids(),
        &[listener_id]
    );
}

#[test]
fn changed_android_proxy_route_lifts_its_bound_listener() {
    let mut persisted = ProxyWorkspace::default();
    let listener_id = persisted.listeners[0].id;
    persisted
        .android_network_profiles
        .push(AndroidNetworkProfile {
            id: "profile-g038".into(),
            name: "Android route".into(),
            target_applications: Vec::new(),
            destination_targets: Vec::new(),
            proxy_routes: vec![AndroidProxyRoute {
                destination: "203.0.113.10".into(),
                ports: vec![443],
                listener_id,
            }],
            confirmed_shared_uids: BTreeSet::new(),
            auto_resume_after_reboot: false,
            weak_network: WeakNetworkProfile::default(),
        });
    let mut candidate = persisted.clone();
    candidate.android_network_profiles[0].proxy_routes[0].ports = vec![8443];

    assert_eq!(
        existing_request(persisted, candidate).affected_listener_ids(),
        &[listener_id]
    );
}

#[test]
fn removed_listener_remains_in_the_affected_runtime_scope() {
    let persisted = ProxyWorkspace::default();
    let listener_id = persisted.listeners[0].id;
    let mut candidate = persisted.clone();
    candidate.listeners.clear();

    assert_eq!(
        existing_request(persisted, candidate).affected_listener_ids(),
        &[listener_id]
    );
}

#[test]
fn existing_target_structural_hash_is_derived_from_the_persisted_workspace() {
    let persisted = ProxyWorkspace::default();
    let mut candidate = persisted.clone();
    candidate.name = "normalized candidate".into();
    let encoded = serde_json::to_vec(&persisted).expect("persisted Workspace encodes");
    let digest = ring::digest::digest(&ring::digest::SHA256, &encoded);
    let mut expected = [0; 32];
    expected.copy_from_slice(digest.as_ref());
    let request = existing_request(persisted, candidate);

    assert_eq!(request.persisted_workspace_structural_hash(), expected);
}

#[test]
fn repeated_exact_package_bindings_are_deduplicated_in_the_capture_scope() {
    let package = ProtocolPackageRef {
        id: ProtocolPackageId::new("au-eftex").unwrap(),
        version: ProtocolPackageVersion::new("1.0.0").unwrap(),
    };
    let mut candidate = ProxyWorkspace::default();
    let mut second = candidate.listeners[0].clone();
    second.id = crate::ListenerId::new();
    candidate.listeners.push(second);
    for listener in &mut candidate.listeners {
        let ListenerDataPlane::Http(settings) = &mut listener.data_plane else {
            panic!("default Listener is HTTP");
        };
        settings.body_processing = HttpBodyProcessing::Protocol {
            package: package.clone(),
        };
    }
    let request = existing_request(ProxyWorkspace::default(), candidate);

    assert_eq!(request.exact_package_refs(), &[package]);
}

#[test]
fn new_target_treats_every_candidate_listener_as_affected() {
    let candidate = ProxyWorkspace::default();
    let listener_ids = candidate
        .listeners
        .iter()
        .map(|listener| listener.id)
        .collect::<Vec<_>>();
    let request = EnvironmentApplyBaselineCaptureRequest {
        target: EnvironmentCommitTarget::New {
            workspace_id: candidate.id.as_uuid(),
            display_name: candidate.name.clone(),
        },
        persisted_workspace: None,
        candidate_workspace: candidate,
        schema_version: 1,
        validation_engine_version: crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
    };

    assert_eq!(request.affected_listener_ids(), listener_ids);
}

#[async_trait]
impl EnvironmentApplyBaselineCapturePort for RecordingCapturePort {
    async fn capture(
        &self,
        request: EnvironmentApplyBaselineCaptureRequest,
    ) -> crate::AppResult<crate::EnvironmentValidatedApplyBaseline> {
        self.requests.lock().unwrap().push(request);
        Err(AppError::new("CAPTURE_RECORDED", "capture recorded"))
    }
}

#[tokio::test]
async fn capture_request_carries_persisted_and_normalized_candidate_authority() {
    let mut persisted = ProxyWorkspace {
        name: "persisted".into(),
        ..ProxyWorkspace::default()
    };
    persisted.revision = intercept_proxy_domain::Revision::new(7);
    let candidate = ProxyWorkspace {
        id: persisted.id,
        name: "candidate".into(),
        ..ProxyWorkspace::default()
    };
    let target = EnvironmentCommitTarget::Existing {
        workspace_id: persisted.id.as_uuid(),
        expected_revision: 7,
    };
    let port = RecordingCapturePort::default();

    let _ = port
        .capture(EnvironmentApplyBaselineCaptureRequest {
            target: target.clone(),
            persisted_workspace: Some(persisted.clone()),
            candidate_workspace: candidate.clone(),
            schema_version: 1,
            validation_engine_version: crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION,
        })
        .await;

    let requests = port.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].target, target);
    assert_eq!(requests[0].persisted_workspace.as_ref(), Some(&persisted));
    assert_eq!(requests[0].candidate_workspace, candidate);
    assert_eq!(requests[0].schema_version, 1);
    assert_eq!(
        requests[0].validation_engine_version,
        crate::ENVIRONMENT_VALIDATION_ENGINE_VERSION
    );
}
