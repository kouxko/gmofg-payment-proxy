use super::*;

#[test]
fn draft_wire_rejects_malformed_typed_values_but_accepts_missing_edit_fields() {
    let listener = full_socket_listener();
    let mut value = serde_json::to_value(ListenerDraft::from(listener)).unwrap();
    value["data_plane"]["settings"]["processing"]["settings"]["package"] = serde_json::Value::Null;
    value["data_plane"]["settings"]["topology"]["settings"]["security"]["downstream_tls"]["server_identity"] =
        serde_json::Value::Null;
    value["data_plane"]["settings"]["topology"]["settings"]["security"]["downstream_tls"]["client_authentication"]
        ["trust"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<ListenerDraft>(value.clone()).is_ok());

    let mut invalid_listener_id = value.clone();
    invalid_listener_id["id"] = serde_json::json!("not-a-uuid");
    assert!(serde_json::from_value::<ListenerDraft>(invalid_listener_id).is_err());

    let mut invalid_package = value.clone();
    invalid_package["data_plane"]["settings"]["processing"]["settings"]["package"] =
        serde_json::json!({ "id": "", "version": "1.0.0" });
    assert!(serde_json::from_value::<ListenerDraft>(invalid_package).is_err());

    let mut invalid_certificate = value;
    invalid_certificate["data_plane"]["settings"]["topology"]["settings"]["security"]["downstream_tls"]
        ["server_identity"] = serde_json::json!("not-a-uuid");
    assert!(serde_json::from_value::<ListenerDraft>(invalid_certificate).is_err());
    assert!(
        serde_json::from_value::<ListenerDraftTransition>(serde_json::json!({
            "type": "future_transition"
        }))
        .is_err()
    );
}

#[tokio::test]
async fn stale_revision_precedes_missing_draft_fields() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let workspace = application
        .workspace_create("Draft CAS".into())
        .await
        .unwrap();
    let mut draft = ListenerDraft::from(workspace.listeners[0].clone());
    draft.data_plane = ListenerDataPlaneDraft::Socket(SocketRelayDraftSettings {
        topology: SocketTopologyDraft::LocalResponder(SocketLocalResponderTopologyDraft {
            downstream_security: SocketDownstreamSecurityDraft::Tls {
                downstream_tls: SocketDownstreamTlsDraftSettings {
                    server_identity: None,
                    client_authentication: DownstreamClientAuthenticationDraft::Required {
                        trust: None,
                    },
                },
            },
        }),
        processing: SocketPayloadProcessingDraft::Scripted(ScriptedSocketProcessingDraft {
            package: None,
        }),
        ..SocketRelayDraftSettings::default()
    });

    let error = application
        .listener_draft_validate(
            workspace.id,
            workspace.revision.get().saturating_sub(1),
            draft.clone(),
            Vec::new(),
        )
        .await
        .expect_err("stale revision must be the first gate");
    assert_eq!(error.view_model.code, "REVISION_CONFLICT");

    let validation = application
        .listener_draft_validate(workspace.id, workspace.revision.get(), draft, Vec::new())
        .await
        .unwrap();
    assert!(!validation.valid);
    assert!(validation.normalized.is_none());
    assert!(
        validation
            .field_errors
            .keys()
            .any(|key| key.ends_with("processing.settings.package"))
    );
    assert!(
        validation
            .field_errors
            .keys()
            .any(|key| key.ends_with("server_identity"))
    );
    assert!(
        validation
            .field_errors
            .keys()
            .any(|key| key.ends_with("client_authentication.trust"))
    );
}

#[tokio::test]
async fn http_client_auth_missing_trust_is_a_draft_field_error() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let workspace = application
        .workspace_create("HTTP draft".into())
        .await
        .unwrap();
    let mut draft = ListenerDraft::from(workspace.listeners[0].clone());
    let ListenerDataPlaneDraft::Http(settings) = &mut draft.data_plane else {
        panic!("new listener uses HTTP defaults");
    };
    settings.downstream_tls.client_authentication =
        DownstreamClientAuthenticationDraft::Optional { trust: None };

    let validation = application
        .listener_draft_validate(workspace.id, workspace.revision.get(), draft, Vec::new())
        .await
        .unwrap();
    assert!(!validation.valid);
    assert!(validation.normalized.is_none());
    assert!(validation.field_errors.contains_key(
        "listeners.0.data_plane.settings.downstream_tls.client_authentication.trust"
    ));
}

#[tokio::test]
async fn strict_domain_errors_also_clear_draft_normalized_workspace() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let workspace = application
        .workspace_create("Domain invalid".into())
        .await
        .unwrap();
    let mut draft = ListenerDraft::from(workspace.listeners[0].clone());
    draft.name.clear();

    let validation = application
        .listener_draft_validate(workspace.id, workspace.revision.get(), draft, Vec::new())
        .await
        .unwrap();
    assert!(!validation.valid);
    assert!(validation.normalized.is_none());
    assert!(validation.field_errors.contains_key("listeners.0.name"));
}

#[tokio::test]
async fn valid_draft_returns_normalized_workspace_and_preserves_old_save_path() {
    let application = application_with_fake_ports(Arc::new(FakePorts::default()));
    let workspace = application
        .workspace_create("Draft valid".into())
        .await
        .unwrap();
    let mut listener = workspace.listeners[0].clone();
    listener.name = "Validated draft".into();

    let validation = application
        .listener_draft_validate(
            workspace.id,
            workspace.revision.get(),
            ListenerDraft::from(listener.clone()),
            Vec::new(),
        )
        .await
        .unwrap();
    assert!(validation.valid);
    let normalized = validation.normalized.expect("valid iff normalized is Some");
    assert_eq!(normalized.listeners[0], listener);

    let saved = application
        .listener_save(workspace.id, workspace.revision.get(), listener, Vec::new())
        .await
        .unwrap();
    assert_eq!(saved.listeners[0].name, "Validated draft");
}
