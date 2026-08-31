use super::*;

#[test]
fn captures_primitive_fields_in_connection_event_order() {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);

    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "ex-1",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:9000",
            protocol = "socket"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "received", direction = "upstream", context_bytes_hex = "00FF10", document_json = r#"{"mti":"0200"}"#, display = "0200");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "processed", direction = "upstream", changes_json = r#"[{"rule_id":"40000000-0000-0000-0000-000000000004","matched":true,"operations":[{"kind":"set","path":"/amount"}]}]"#, changes_truncated = true, final_document_json = r#"{"amount":120}"#);
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "encoded", protocol = "socket", direction = "upstream", context_bytes_hex = "0102");
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "closed", outcome = "completed");
    });
    consumer.shutdown().unwrap();

    let record = wait_for_record(&store, "ex-1", 5);
    assert_eq!(record.events.len(), 5);
    let ExchangeObservationEvent::Received {
        context, document, ..
    } = &record.events[1]
    else {
        panic!("second event must be received");
    };
    assert_eq!(
        context,
        &ExchangeContext::Socket {
            bytes: vec![0, 255, 16]
        }
    );
    assert_eq!(document.as_ref().expect("protocol document")["mti"], "0200");
    let ExchangeObservationEvent::Processed {
        changes,
        changes_truncated,
        final_document,
        ..
    } = &record.events[2]
    else {
        panic!("third event must be processed");
    };
    assert_eq!(changes.len(), 1);
    assert!(changes[0].matched);
    assert_eq!(changes[0].operations[0].path.as_deref(), Some("/amount"));
    assert!(*changes_truncated);
    assert_eq!(final_document["amount"], 120);
    assert!(matches!(
        record.events[3],
        ExchangeObservationEvent::Encoded { .. }
    ));
}
