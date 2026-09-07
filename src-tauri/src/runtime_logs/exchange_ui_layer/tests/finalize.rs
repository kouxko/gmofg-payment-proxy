use super::*;
use intercept_proxy_application::ExternalPackageCallStage;

#[test]
fn http_finalize_failure_reaches_the_observation_store_with_package_details() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);
    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "finalize-rejected",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:8080",
            protocol = "http"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        tracing::error!(
            target: "intercept_proxy::exchange::ui",
            event = "failed",
            direction = "downstream",
            stage = "finalize",
            context_header = "HTTP/1.1 200 OK\r\n\r\n",
            context_body = "prepared body",
            context_body_is_utf8 = true,
            error = "method must remain unchanged",
            external_package_id = "http-finalize-test",
            external_package_version = "1.0.0",
            external_stage = "finalize",
            external_method = "downstream-finalize",
            external_stable_code = "FINALIZE_REJECTED"
        );
    });
    consumer.shutdown().unwrap();

    let record = wait_for_record(&store, "finalize-rejected", 2);
    let ExchangeObservationEvent::Failed {
        stage,
        context,
        external_package_call,
        ..
    } = &record.events[1]
    else {
        panic!("expected a failed observation");
    };
    assert_eq!(stage, "finalize");
    let call = external_package_call
        .as_ref()
        .expect("Finalize package details");
    assert_eq!(call.stage, ExternalPackageCallStage::Finalize);
    assert_eq!(call.method, "downstream-finalize");
    assert_eq!(call.stable_code.as_deref(), Some("FINALIZE_REJECTED"));
    assert!(matches!(context, Some(ExchangeContext::Http { body, .. }) if body == "prepared body"));
}
