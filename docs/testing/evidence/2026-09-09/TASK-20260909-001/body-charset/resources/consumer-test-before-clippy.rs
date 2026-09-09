use super::*;

const RECEIPT: &str = "売上票（署名）保管正常完了。";
const RECEIPT_BYTES: &[u8] = include_bytes!("fixtures/receipt-shift-jis.bin");

fn observe_http_body(header: &str, bytes: &[u8]) -> Vec<ExchangeContext> {
    let store = Arc::new(ExchangeObservationStore::new(Arc::new(
        CapacityLedger::new(64 * 1024),
    )));
    let (layer, consumer) = layer(&store);
    let subscriber = tracing_subscriber::registry().with(layer);
    let hex = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let lossy = String::from_utf8_lossy(bytes);
    with_default(subscriber, || {
        let span = tracing::info_span!(
            "exchange",
            exchange_id = "charset-preview",
            workspace_id = "10000000-0000-0000-0000-000000000001",
            listener_id = "20000000-0000-0000-0000-000000000002",
            runtime_epoch = "30000000-0000-0000-0000-000000000003",
            peer = "127.0.0.1:8080",
            protocol = "http"
        );
        let _entered = span.enter();
        tracing::info!(target: "intercept_proxy::exchange::ui", event = "opened");
        for event in ["received", "encoded", "sent", "failed"] {
            tracing::info!(
                target: "intercept_proxy::exchange::ui", event, direction = "downstream",
                context_header = header, context_body = lossy.as_ref(),
                context_body_is_utf8 = std::str::from_utf8(bytes).is_ok(),
                context_bytes_hex = hex.as_str(), stage = "display", error = "test failure"
            );
        }
    });
    consumer.shutdown().unwrap();
    let record = store.get("charset-preview").unwrap();
    assert_eq!(record.events.len(), 5);
    println!(
        "HTTP_PREVIEW_RECORD {}",
        serde_json::to_string(&record).unwrap()
    );
    record
        .events
        .into_iter()
        .skip(1)
        .map(|event| match event {
            ExchangeObservationEvent::Received { context, .. }
            | ExchangeObservationEvent::Encoded { context, .. }
            | ExchangeObservationEvent::Sent { context, .. }
            | ExchangeObservationEvent::Failed {
                context: Some(context),
                ..
            } => context,
            _ => panic!("expected an HTTP context"),
        })
        .collect()
}

#[test]
fn shift_jis_body_preview_is_readable_in_all_stages_but_cannot_seed_utf8_rules() {
    let header = "HTTP/1.1 200 OK\r\nContent-Type: text/csv;charset=Shift_JIS\r\n\r\n";
    for context in observe_http_body(header, RECEIPT_BYTES) {
        assert_eq!(
            context,
            ExchangeContext::Http {
                header: header.to_owned(),
                body: RECEIPT.to_owned(),
                body_is_utf8: false,
            }
        );
    }
    assert!(String::from_utf8_lossy(RECEIPT_BYTES).contains('\u{fffd}'));
}

#[test]
fn missing_unsupported_and_compressed_charsets_keep_existing_lossy_observation() {
    for headers in [
        "",
        "Content-Type: text/plain\r\n",
        "Content-Type: text/plain;charset=unknown\r\n",
        "Content-Type: text/plain;charset=Shift_JIS\r\nContent-Encoding: gzip\r\n",
    ] {
        let header = format!("HTTP/1.1 200 OK\r\n{headers}\r\n");
        for context in observe_http_body(&header, RECEIPT_BYTES) {
            assert!(
                matches!(context, ExchangeContext::Http { body, body_is_utf8: false, .. }
                if body == String::from_utf8_lossy(RECEIPT_BYTES))
            );
        }
    }
}

#[test]
fn invalid_declared_charset_reports_preview_failure_without_changing_event_outcome() {
    for charset in ["Shift_JIS", "UTF-8"] {
        let header =
            format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain;charset={charset}\r\n\r\n");
        for context in observe_http_body(&header, &[0x82]) {
            assert!(
                matches!(context, ExchangeContext::Http { body, body_is_utf8: false, .. }
                if body.contains("Body 预览解码失败") && body.contains("DECODE_FAILED"))
            );
        }
    }
}

#[test]
fn utf8_body_preview_keeps_existing_text_and_rule_eligibility() {
    for context in observe_http_body("HTTP/1.1 200 OK\r\n\r\n", RECEIPT.as_bytes()) {
        assert!(
            matches!(context, ExchangeContext::Http { body, body_is_utf8: true, .. } if body == RECEIPT)
        );
    }
}
