use std::sync::{Arc, Mutex};

use tracing::{Event, Subscriber, field::Visit, subscriber::with_default};
use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

use super::{MAX_OBSERVATION_TEXT_BYTES, failed, failed_with_context, raw_failed, raw_received};
use crate::{
    Error, ExternalPackageCallFailure, ExternalPackageCallStage, Http, HttpContext,
    ProtocolDirection, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
    RuleProcessingAccumulator, RuleProcessingChange, RuleProcessingOperation,
    RuleProcessingOperationKind, Socket, SocketContext, Upstream, rules_processed,
};

#[derive(Default)]
struct CapturedFields {
    event: Option<String>,
    protocol: Option<String>,
    context_header: Option<String>,
    context_body: Option<String>,
    context_body_is_utf8: Option<bool>,
    context_bytes: Option<String>,
    external_method: Option<String>,
    external_request_id: Option<String>,
    external_stable_code: Option<String>,
    external_remote_code: Option<i64>,
    external_package_id: Option<String>,
    external_package_version: Option<String>,
    external_stage: Option<String>,
    external_remote_message: Option<String>,
    external_remote_data_summary: Option<String>,
    changes_truncated: Option<bool>,
}

impl Visit for CapturedFields {
    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "event" => self.event = Some(value.to_owned()),
            "protocol" => self.protocol = Some(value.to_owned()),
            "context_header" => self.context_header = Some(value.to_owned()),
            "context_body" => self.context_body = Some(value.to_owned()),
            "context_bytes_hex" => self.context_bytes = Some(value.to_owned()),
            "external_method" => self.external_method = Some(value.to_owned()),
            "external_request_id" => self.external_request_id = Some(value.to_owned()),
            "external_stable_code" => self.external_stable_code = Some(value.to_owned()),
            "external_package_id" => self.external_package_id = Some(value.to_owned()),
            "external_package_version" => self.external_package_version = Some(value.to_owned()),
            "external_stage" => self.external_stage = Some(value.to_owned()),
            "external_remote_message" => self.external_remote_message = Some(value.to_owned()),
            "external_remote_data_summary" => {
                self.external_remote_data_summary = Some(value.to_owned());
            }
            _ => {}
        }
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if field.name() == "external_remote_code" {
            self.external_remote_code = Some(value);
        }
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "changes_truncated" {
            self.changes_truncated = Some(value);
        }
        if field.name() == "context_body_is_utf8" {
            self.context_body_is_utf8 = Some(value);
        }
    }
}

struct CaptureLayer(Arc<Mutex<CapturedFields>>);

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut fields = CapturedFields::default();
        event.record(&mut fields);
        *self.0.lock().unwrap() = fields;
    }
}

#[test]
fn raw_failure_without_bytes_omits_context_instead_of_forging_empty_frame() {
    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    with_default(subscriber, || {
        raw_failed::<Upstream>("read", None, &Error::new("peer closed"));
    });
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("failed"));
    assert!(captured.context_bytes.is_none());
}

#[test]
fn oversized_raw_payload_emits_fixed_loss_without_building_hex_projection() {
    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    let bytes = vec![0xA5; MAX_OBSERVATION_TEXT_BYTES / 2 + 1];
    with_default(subscriber, || raw_received::<Upstream>(&bytes));
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("observation_lost"));
    assert!(captured.context_bytes.is_none());
}

#[test]
fn fail_open_display_observation_keeps_typed_external_failure_fields() {
    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    let error = Error::new("EXTERNAL_PACKAGE_CALL_FAILED\ndisplay rejected")
        .with_external_package_call(ExternalPackageCallFailure {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("phase10.http").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            direction: ProtocolDirection::Upstream,
            stage: ExternalPackageCallStage::Display,
            method: "document.upstream.display".into(),
            request_id: Some("display-observation-1".into()),
            remote_code: Some(-32_409),
            stable_code: Some("INTERNAL_ERROR".into()),
            remote_message: Some("display rejected".into()),
            remote_data_summary: Some("object(fields=1)".into()),
        });
    let context = HttpContext {
        header: "POST / HTTP/1.1\r\n\r\n".into(),
        body: "wire".into(),
        body_is_utf8: true,
        wire_body: b"wire".to_vec(),
    };
    with_default(subscriber, || {
        failed_with_context::<Http, Upstream>("display", &context, &error);
    });
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("failed"));
    assert_eq!(
        captured.external_method.as_deref(),
        Some("document.upstream.display")
    );
    assert_eq!(
        captured.external_request_id.as_deref(),
        Some("display-observation-1")
    );
    assert_eq!(captured.external_remote_code, Some(-32_409));
    assert_eq!(
        captured.external_stable_code.as_deref(),
        Some("INTERNAL_ERROR")
    );
}

#[test]
fn socket_display_failure_keeps_context_and_all_external_correlation_fields() {
    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    let error =
        Error::new("display rejected").with_external_package_call(ExternalPackageCallFailure {
            package: ProtocolPackageRef {
                id: ProtocolPackageId::new("socket.external").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            direction: ProtocolDirection::Upstream,
            stage: ExternalPackageCallStage::Display,
            method: "document.upstream.display".into(),
            request_id: Some("socket-display-1".into()),
            remote_code: Some(-32_409),
            stable_code: Some("INTERNAL_ERROR".into()),
            remote_message: Some("display rejected".into()),
            remote_data_summary: Some("object(fields=1)".into()),
        });
    with_default(subscriber, || {
        failed_with_context::<Socket, Upstream>(
            "display",
            &SocketContext {
                data: b"wire".to_vec(),
            },
            &error,
        );
    });
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("failed"));
    assert_eq!(captured.context_bytes.as_deref(), Some("77697265"));
    assert_eq!(
        captured.external_package_id.as_deref(),
        Some("socket.external")
    );
    assert_eq!(captured.external_package_version.as_deref(), Some("1.0.0"));
    assert_eq!(captured.external_stage.as_deref(), Some("display"));
    assert_eq!(
        captured.external_method.as_deref(),
        Some("document.upstream.display")
    );
    assert_eq!(
        captured.external_request_id.as_deref(),
        Some("socket-display-1")
    );
    assert_eq!(captured.external_remote_code, Some(-32_409));
    assert_eq!(
        captured.external_stable_code.as_deref(),
        Some("INTERNAL_ERROR")
    );
    assert_eq!(
        captured.external_remote_message.as_deref(),
        Some("display rejected")
    );
    assert_eq!(
        captured.external_remote_data_summary.as_deref(),
        Some("object(fields=1)")
    );
}

fn external_budget_error(message: String) -> Error {
    Error::new("remote failure").with_external_package_call(ExternalPackageCallFailure {
        package: ProtocolPackageRef {
            id: ProtocolPackageId::new("socket.external").unwrap(),
            version: ProtocolPackageVersion::new("1.0.0").unwrap(),
        },
        direction: ProtocolDirection::Upstream,
        stage: ExternalPackageCallStage::Display,
        method: "document.upstream.display".into(),
        request_id: Some("budget-request".into()),
        remote_code: Some(-1),
        stable_code: Some("INTERNAL_ERROR".into()),
        remote_message: Some(message),
        remote_data_summary: Some("字段".into()),
    })
}

#[test]
fn socket_external_fields_share_the_context_projection_budget() {
    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    let error = external_budget_error("x".repeat(5 * 1024 * 1024));
    with_default(subscriber, || {
        failed_with_context::<Socket, Upstream>(
            "display",
            &SocketContext {
                data: vec![0xA5; 6 * 1024 * 1024],
            },
            &error,
        );
    });
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("observation_lost"));
    assert!(captured.context_bytes.is_none());
    assert!(captured.external_remote_message.is_none());
}

#[test]
fn all_failure_projections_accept_exact_budget_and_reject_one_extra_byte() {
    let fixed_error_bytes: usize = [
        "remote failure",
        "socket.external",
        "1.0.0",
        "display",
        "document.upstream.display",
        "budget-request",
        "INTERNAL_ERROR",
        "字段",
    ]
    .iter()
    .map(|value| value.len())
    .sum();
    let http = HttpContext {
        header: "HTTP/1.1 500\r\nContent-Length: 4\r\n\r\n".into(),
        body: "wire".into(),
        body_is_utf8: true,
        wire_body: b"wire".to_vec(),
    };
    let socket = SocketContext {
        data: b"wire".to_vec(),
    };
    for projection in ["http", "socket", "without_context"] {
        let context_bytes = match projection {
            "http" => http.header.len() + http.body.len(),
            "socket" => socket.data.len() * 2,
            _ => 0,
        };
        for extra in [0, 1] {
            let captured = Arc::new(Mutex::new(CapturedFields::default()));
            let subscriber =
                tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
            let error = external_budget_error(
                "x".repeat(MAX_OBSERVATION_TEXT_BYTES - fixed_error_bytes - context_bytes + extra),
            );
            with_default(subscriber, || match projection {
                "http" => failed_with_context::<Http, Upstream>("display", &http, &error),
                "socket" => failed_with_context::<Socket, Upstream>("display", &socket, &error),
                _ => failed::<Upstream>("display", &error),
            });
            let captured = captured.lock().unwrap();
            let expected_event = if extra == 0 {
                "failed"
            } else {
                "observation_lost"
            };
            eprintln!(
                "projection={projection}; content_budget={MAX_OBSERVATION_TEXT_BYTES}; extra={extra}; event={:?}",
                captured.event
            );
            assert_eq!(
                captured.event.as_deref(),
                Some(expected_event),
                "{projection}, extra={extra}"
            );
            if extra == 0 {
                assert_eq!(
                    captured.external_method.as_deref(),
                    Some("document.upstream.display")
                );
                assert_eq!(
                    captured.external_remote_data_summary.as_deref(),
                    Some("字段")
                );
                match projection {
                    "http" => {
                        assert_eq!(captured.protocol.as_deref(), Some("http"));
                        assert_eq!(
                            captured.context_header.as_deref(),
                            Some(http.header.as_str())
                        );
                        assert_eq!(captured.context_body.as_deref(), Some("wire"));
                        assert_eq!(captured.context_body_is_utf8, Some(true));
                        assert!(captured.context_bytes.is_none());
                    }
                    "socket" => {
                        assert_eq!(captured.protocol.as_deref(), Some("socket"));
                        assert_eq!(captured.context_bytes.as_deref(), Some("77697265"));
                        assert!(captured.context_header.is_none());
                        assert!(captured.context_body.is_none());
                    }
                    _ => {
                        assert!(captured.protocol.is_none());
                        assert!(captured.context_header.is_none());
                        assert!(captured.context_bytes.is_none());
                    }
                }
            } else {
                assert!(captured.external_method.is_none());
                assert!(captured.context_bytes.is_none());
                assert!(captured.context_header.is_none());
            }
        }
    }
}

#[test]
fn large_document_and_1024_rules_keep_process_evidence_within_the_fixed_budget() {
    let payload = "x".repeat(4 * 1024 * 1024);
    let document = crate::Document::parse_json(
        &serde_json::to_string(&serde_json::json!({ "payload": payload })).unwrap(),
    )
    .unwrap();
    let mut accumulator = RuleProcessingAccumulator::default();
    for index in 0..1024 {
        accumulator.record(RuleProcessingChange {
            rule_id: format!("rule-{index}-{}", "p".repeat(32 * 1024)),
            matched: true,
            operations: vec![RuleProcessingOperation {
                kind: RuleProcessingOperationKind::Set,
                path: Some(format!("/field/{index}")),
            }],
        });
    }

    assert!(accumulator.truncated());
    assert!(accumulator.retained_bytes() <= MAX_OBSERVATION_TEXT_BYTES);
    assert_eq!(document.to_json().unwrap().len(), 4 * 1024 * 1024 + 14);

    let captured = Arc::new(Mutex::new(CapturedFields::default()));
    let subscriber = tracing_subscriber::registry().with(CaptureLayer(Arc::clone(&captured)));
    with_default(subscriber, || {
        rules_processed(ProtocolDirection::Upstream, &accumulator, &document);
    });
    let captured = captured.lock().unwrap();
    assert_eq!(captured.event.as_deref(), Some("processed"));
    assert_eq!(captured.changes_truncated, Some(true));
}
