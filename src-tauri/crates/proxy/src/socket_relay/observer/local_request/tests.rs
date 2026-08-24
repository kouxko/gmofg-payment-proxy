//! request preview 的截断、脱敏 Debug 与双维度队列门禁。

use std::{net::SocketAddr, time::SystemTime};

use uuid::Uuid;

use super::{
    LOCAL_REQUEST_DOCUMENT_PREVIEW_MAX_BYTES, LOCAL_REQUEST_ORIGIN_PREVIEW_MAX_BYTES,
    SocketDocumentFieldPreview, SocketDocumentPreview, SocketLocalRequestPreview,
};
use crate::socket_relay::{
    BoundedSocketConnectionObserver, SocketConnectionEvent, SocketConnectionObserver,
    SocketConnectionTarget, SocketRelayRunContext, SocketTransportMode,
};

#[test]
fn origin_and_document_previews_stay_within_their_exact_budgets() {
    let origin = vec![0xAB; LOCAL_REQUEST_ORIGIN_PREVIEW_MAX_BYTES + 1];
    let document = SocketDocumentPreview::new(
        "schema".into(),
        "1".into(),
        vec![SocketDocumentFieldPreview {
            name: "memo".into(),
            label: "备注".into(),
            field_type: "string".into(),
            present: true,
            value: Some("界".repeat(LOCAL_REQUEST_DOCUMENT_PREVIEW_MAX_BYTES)),
            value_truncated: false,
            value_omitted: false,
        }],
    );
    let preview = SocketLocalRequestPreview::new(Uuid::new_v4(), &origin, Some(document));

    assert_eq!(
        preview.origin_preview.len(),
        LOCAL_REQUEST_ORIGIN_PREVIEW_MAX_BYTES
    );
    assert!(preview.origin_truncated);
    let document = preview.document.unwrap();
    assert!(document.truncated);
    assert!(document.logical_bytes() <= LOCAL_REQUEST_DOCUMENT_PREVIEW_MAX_BYTES);
    assert!(document.fields[0].value_truncated);
    assert!(
        document.fields[0]
            .value
            .as_ref()
            .unwrap()
            .is_char_boundary(document.fields[0].value.as_ref().unwrap().len())
    );
}

#[test]
fn debug_never_prints_origin_or_document_values() {
    let preview = SocketLocalRequestPreview::new(
        Uuid::new_v4(),
        b"secret-origin",
        Some(SocketDocumentPreview::new(
            "schema".into(),
            "1".into(),
            vec![SocketDocumentFieldPreview {
                name: "pin".into(),
                label: "PIN".into(),
                field_type: "string".into(),
                present: true,
                value: Some("secret-value".into()),
                value_truncated: false,
                value_omitted: false,
            }],
        )),
    );

    let debug = format!("{preview:?}");
    assert!(!debug.contains("secret-origin"));
    assert!(!debug.contains("secret-value"));
}

#[test]
fn observer_evicts_oldest_event_by_count_and_logical_bytes() {
    let observer = BoundedSocketConnectionObserver::with_limits(2, 1_024).unwrap();
    let run = SocketRelayRunContext {
        workspace_id: "test-workspace".into(),
        listener_id: "listener".into(),
        workspace_runtime_epoch: Uuid::new_v4(),
        listener_run_epoch: Uuid::new_v4(),
    };
    for _ in 0..3 {
        observer.record(SocketConnectionEvent::Admitted {
            run: run.clone(),
            connection_id: Uuid::new_v4(),
            peer: "127.0.0.1:9000".parse::<SocketAddr>().unwrap(),
            target: SocketConnectionTarget::LocalResponder,
            mode: SocketTransportMode::Transparent,
            at: SystemTime::now(),
        });
    }

    assert_eq!(observer.snapshot().len(), 2);
    assert_eq!(observer.retained_diagnostic_evictions(), 1);

    observer.record(SocketConnectionEvent::RequestParsed {
        run,
        connection_id: Uuid::new_v4(),
        preview: SocketLocalRequestPreview::new(Uuid::new_v4(), &[7; 2_048], None),
        at: SystemTime::now(),
    });
    assert!(observer.snapshot().is_empty());
    assert_eq!(observer.retained_diagnostic_evictions(), 4);
}
