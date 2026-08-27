use intercept_proxy_application::AppError;
use intercept_proxy_domain::ListenerId;

use super::*;

fn diagnostic_registry() -> (ExternalPackageRegistryAdapter, Arc<EventHub>) {
    let events = Arc::new(EventHub::new(32));
    let registry = ExternalPackageRegistryAdapter::new(Arc::new(SqliteStore::in_memory().unwrap()));
    registry.set_event_hub(Arc::clone(&events));
    (registry, events)
}

fn rendered_diagnostics(events: &EventHub) -> String {
    events
        .diagnostic_log_snapshot()
        .iter()
        .map(|row| {
            format!(
                "{} {}",
                row.summary,
                row.detail.as_deref().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn connection_attempt_failures_are_queryable_without_secret_values() {
    let (registry, events) = diagnostic_registry();
    let remote_address = "127.0.0.1:49061".parse().unwrap();

    registry
        .mark_service_failed(
            "ws://127.0.0.1:9000/packages",
            "password=do-not-leak; bind failed",
        )
        .await;
    registry.record_connection_attempt_failure(
        "websocket_handshake",
        remote_address,
        "EXTERNAL_PACKAGE_HANDSHAKE_REJECTED",
    );
    registry.record_registration_failure(
        remote_address,
        &ExternalPackageConnectionError::Disconnected,
    );

    let rendered = rendered_diagnostics(&events);
    assert!(rendered.contains("EXTERNAL_PACKAGE_SERVICE_BIND_FAILED"));
    assert!(rendered.contains("EXTERNAL_PACKAGE_HANDSHAKE_REJECTED"));
    assert!(rendered.contains("EXTERNAL_PACKAGE_DISCONNECTED"));
    assert!(!rendered.contains("do-not-leak"));
}

#[test]
fn package_processing_failures_include_stable_correlation_fields() {
    let (registry, events) = diagnostic_registry();
    let package = registration("Vendor ISO8583").package().identity().clone();
    let listener_id = ListenerId::new();
    let error = AppError::new("SAFE_FAILURE", "password=do-not-leak; operation failed");

    registry.record_application_failure(
        "identity",
        "127.0.0.1:49062".parse().unwrap(),
        Some(&package),
        &error,
    );
    registry.record_listener_stop_failure(&package, listener_id, &error);
    registry.record_listener_stopped_after_disconnect(&package, listener_id);
    registry.record_package_operation_failure("usage_query", &package, &error);

    let rendered = rendered_diagnostics(&events);
    assert!(rendered.contains("vendor-iso8583@1.0.0"));
    assert!(rendered.contains(&listener_id.to_string()));
    assert!(rendered.contains("SAFE_FAILURE"));
    assert!(rendered.contains("listener_stopped_after_external_package_offline"));
    assert!(!rendered.contains("do-not-leak"));
}

#[test]
fn clean_disconnect_is_reported_as_a_stable_offline_event() {
    let (registry, events) = diagnostic_registry();
    let package = registration("Vendor ISO8583").package().identity().clone();
    let connection_id = ExternalPackageConnectionId(uuid::Uuid::new_v4());

    registry.publish_connection_offline(
        &package,
        connection_id,
        &ExternalPackageConnectionError::Disconnected,
    );

    let rendered = rendered_diagnostics(&events);
    assert!(rendered.contains("event=disconnected"));
    assert!(rendered.contains("EXTERNAL_PACKAGE_DISCONNECTED"));
    assert!(rendered.contains(&connection_id.as_uuid().to_string()));
}
