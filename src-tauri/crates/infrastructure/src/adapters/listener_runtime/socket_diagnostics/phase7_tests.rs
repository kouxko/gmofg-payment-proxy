use std::{sync::Arc, time::SystemTime};

use intercept_proxy_application::{EventHub, UiEventPayload};
use intercept_proxy_domain::{
    ProtocolDirection, ProtocolPackageId, ProtocolPackageRef, ProtocolPackageVersion,
};
use intercept_proxy_exchange::{ExternalPackageCallFailure, ExternalPackageCallStage};
use intercept_proxy_runtime::{
    SocketConnectionEvent, SocketConnectionObserver, SocketConnectionTarget, SocketRelayBytes,
    SocketRelayDirection, SocketRelayFailure, SocketRelayStage,
};
use uuid::Uuid;

use super::{SocketDiagnosticObserver, tests::run};

#[test]
fn active_socket_failure_exposes_external_package_stable_code() {
    let events = Arc::new(EventHub::new(16));
    let observer = SocketDiagnosticObserver::new(Arc::clone(&events), 16, 64 * 1024).unwrap();
    observer.record(SocketConnectionEvent::Closed {
        run: run(),
        connection_id: Uuid::new_v4(),
        target: SocketConnectionTarget::LocalResponder,
        opened: true,
        bytes: SocketRelayBytes::default(),
        failure: Some(SocketRelayFailure {
            stage: SocketRelayStage::Decode,
            direction: Some(SocketRelayDirection::ClientToServer),
            code: "DECODE_FAILED",
            external_package_call: Some(Box::new(ExternalPackageCallFailure {
                package: ProtocolPackageRef {
                    id: ProtocolPackageId::new("diagnostic-package").unwrap(),
                    version: ProtocolPackageVersion::new("1.0.0").unwrap(),
                },
                direction: ProtocolDirection::Upstream,
                stage: ExternalPackageCallStage::Decode,
                method: "upstream.decode".into(),
                request_id: Some("rpc-7".into()),
                remote_code: Some(-32001),
                stable_code: Some("PROTOCOL_PACKAGE_INVALID".into()),
                remote_message: Some("rejected".into()),
                remote_data_summary: Some("object(fields=1)".into()),
            })),
        }),
        at: SystemTime::now(),
    });
    let replay = events.replay_after(0);
    let UiEventPayload::DiagnosticLogAdded(entry) = &replay.events[0].payload else {
        panic!("expected diagnostic");
    };
    assert_eq!(
        entry
            .socket_context
            .as_ref()
            .unwrap()
            .external_package_call
            .as_ref()
            .unwrap()
            .stable_code
            .as_deref(),
        Some("PROTOCOL_PACKAGE_INVALID")
    );
}
