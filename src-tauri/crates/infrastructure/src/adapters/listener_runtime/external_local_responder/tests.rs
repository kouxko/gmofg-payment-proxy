use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use intercept_proxy_domain::{
    DocumentAction, DocumentFieldName, DocumentValue, ExternalDecodeResponse,
    ExternalDisplayResponse, ExternalEncodeResponse, ExternalFrameResult,
    ExternalPackageRegistration, ListenerId, ProtocolDocumentRuleDefinition,
    ProtocolDocumentRuleId, ProtocolDocumentRuleProgram, SocketTopology,
};
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::{Semaphore, mpsc};
use uuid::Uuid;

use super::*;
use crate::adapters::{
    external_packages::ExternalPackageConnectionError,
    listener_runtime::{
        ProtocolDocumentRuleConnectionFactory,
        external_relay::contract::{ExternalPackageRpc, ExternalSocketPackageBinding},
    },
};

mod coverage;

#[tokio::test]
async fn local_exchange_uses_upstream_decode_request_rule_and_downstream_response_rule_encode() {
    let registration = registration();
    let rpc = Arc::new(FakeRpc::default());
    let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc.clone()),
        rules(&registration),
        SocketTopology::default(),
    ));
    let factory = ExternalLocalResponderProcessorFactoryAdapter::new(
        snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    );
    let mut processor = factory.create_exchange(SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    });

    assert_eq!(
        processor
            .process(Bytes::from_static(b"sale"))
            .await
            .unwrap(),
        Bytes::from_static(b"approved")
    );
    assert_eq!(
        serde_json::to_value(rpc.encoded.lock().clone().unwrap()).unwrap()["response_code"],
        json!({"type": "string", "value": "00"})
    );
    assert_eq!(
        rpc.calls.lock().as_slice(),
        ["hooks.upstream.decode", "hooks.downstream.encode"]
    );
}

#[tokio::test]
async fn exchanges_on_one_business_connection_serialize_display_work() {
    let registration = registration();
    let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
    let release_first = Arc::new(Semaphore::new(0));
    let rpc = Arc::new(FakeRpc {
        display_probe: Some(DisplayProbe {
            entered: entered_tx,
            release_first: Arc::clone(&release_first),
        }),
        ..FakeRpc::default()
    });
    let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc),
        rules(&registration),
        SocketTopology::default(),
    ));
    let factory = ExternalLocalResponderProcessorFactoryAdapter::new(
        snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    );
    let connection = SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    };
    let mut first = factory.create_exchange(connection.clone());
    let mut second = factory.create_exchange(connection);

    first.process(Bytes::from_static(b"one")).await.unwrap();
    first.output_committed();
    second.process(Bytes::from_static(b"two")).await.unwrap();
    second.output_committed();

    assert_eq!(entered_rx.recv().await, Some(1));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), entered_rx.recv())
            .await
            .is_err(),
        "the next exchange must not overtake the first exchange display"
    );
    release_first.add_permits(1);
    assert_eq!(entered_rx.recv().await, Some(2));
    assert_eq!(entered_rx.recv().await, Some(3));
    assert_eq!(entered_rx.recv().await, Some(4));
}

#[tokio::test]
async fn failed_output_capture_stays_ordered_before_the_next_exchange() {
    let (factory, release_first, mut entered_rx, connection) = probed_factory();
    let mut failed = factory.create_exchange(connection.clone());
    let mut committed = factory.create_exchange(connection);

    failed.process(Bytes::from_static(b"one")).await.unwrap();
    failed.output_failed(
        &SocketProcessingFailure::new(
            SocketProcessingFailureKind::WriteFailed,
            "test write failure",
        ),
        3,
    );
    committed.process(Bytes::from_static(b"two")).await.unwrap();
    committed.output_committed();

    assert_eq!(entered_rx.recv().await, Some(1));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), entered_rx.recv())
            .await
            .is_err(),
        "a committed exchange must not overtake failed-output capture work"
    );
    release_first.add_permits(1);
    assert_eq!(entered_rx.recv().await, Some(2));
    assert_eq!(entered_rx.recv().await, Some(3));
}

#[tokio::test]
async fn display_failure_is_fail_open_after_response_bytes_are_fixed() {
    let registration = registration();
    let (entered_tx, mut entered_rx) = mpsc::unbounded_channel();
    let rpc = Arc::new(FakeRpc {
        display_probe: Some(DisplayProbe {
            entered: entered_tx,
            release_first: Arc::new(Semaphore::new(1)),
        }),
        fail_display: true,
        ..FakeRpc::default()
    });
    let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc),
        rules(&registration),
        SocketTopology::default(),
    ));
    let factory = ExternalLocalResponderProcessorFactoryAdapter::new(
        snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    );
    let mut processor = factory.create_exchange(SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    });

    let written = processor
        .process(Bytes::from_static(b"sale"))
        .await
        .unwrap();
    processor.output_committed();

    assert_eq!(written, Bytes::from_static(b"approved"));
    assert_eq!(entered_rx.recv().await, Some(1));
    assert_eq!(entered_rx.recv().await, Some(2));
}

#[tokio::test]
async fn display_failure_returns_hex_with_queryable_external_call_diagnostic() {
    let rpc = FakeRpc {
        fail_display: true,
        ..FakeRpc::default()
    };
    let package = registration().package().identity().clone();
    let connection = SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    };
    let capture = SocketCaptureContext {
        workspace_id: intercept_proxy_domain::WorkspaceId::new(),
        listener_id: listener_id(),
        publisher: None,
    };
    let document = serde_json::from_value(json!({
        "request": {"type": "string", "value": "sale"}
    }))
    .unwrap();

    let result = display(
        &rpc,
        &package,
        &connection,
        ProtocolDirection::Upstream,
        "document.upstream.display",
        document,
        &capture,
    )
    .await;

    let SocketDisplayResult::HexFallback {
        reason,
        diagnostic: Some(diagnostic),
    } = result
    else {
        panic!("display failure must return a diagnostic Hex fallback");
    };
    assert_eq!(reason, SocketDisplayFallbackReason::EntryPointFailed);
    let external = diagnostic.external_package_call.unwrap();
    assert_eq!(external.stage, ExternalPackageCallStage::Display);
    assert_eq!(external.method, "document.upstream.display");
    assert_eq!(external.package, package);
}

#[test]
fn factory_removes_dead_business_connection_lanes() {
    let registration = registration();
    let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), Arc::new(FakeRpc::default())),
        rules(&registration),
        SocketTopology::default(),
    ));
    let factory = ExternalLocalResponderProcessorFactoryAdapter::new(
        snapshot,
        SocketCaptureContext {
            workspace_id: intercept_proxy_domain::WorkspaceId::new(),
            listener_id: listener_id(),
            publisher: None,
        },
    );
    let first_connection = SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(2),
        peer_addr: "127.0.0.1:12345".parse().unwrap(),
    };
    drop(factory.create_exchange(first_connection));
    assert_eq!(factory.display_lanes.lock().len(), 1);

    let second_connection = SocketConnectionIdentity {
        runtime_epoch: Uuid::from_u128(1),
        connection_id: Uuid::from_u128(3),
        peer_addr: "127.0.0.1:12346".parse().unwrap(),
    };
    let _active = factory.create_exchange(second_connection);

    assert_eq!(factory.display_lanes.lock().len(), 1);
}

fn probed_factory() -> (
    ExternalLocalResponderProcessorFactoryAdapter,
    Arc<Semaphore>,
    mpsc::UnboundedReceiver<usize>,
    SocketConnectionIdentity,
) {
    let registration = registration();
    let (entered_tx, entered_rx) = mpsc::unbounded_channel();
    let release_first = Arc::new(Semaphore::new(0));
    let rpc = Arc::new(FakeRpc {
        display_probe: Some(DisplayProbe {
            entered: entered_tx,
            release_first: Arc::clone(&release_first),
        }),
        ..FakeRpc::default()
    });
    let snapshot = Arc::new(ExternalSocketRuntimeSnapshot::new(
        ExternalSocketPackageBinding::new(registration.clone(), rpc),
        rules(&registration),
        SocketTopology::default(),
    ));
    (
        ExternalLocalResponderProcessorFactoryAdapter::new(
            snapshot,
            SocketCaptureContext {
                workspace_id: intercept_proxy_domain::WorkspaceId::new(),
                listener_id: listener_id(),
                publisher: None,
            },
        ),
        release_first,
        entered_rx,
        SocketConnectionIdentity {
            runtime_epoch: Uuid::from_u128(1),
            connection_id: Uuid::from_u128(2),
            peer_addr: "127.0.0.1:12345".parse().unwrap(),
        },
    )
}

#[derive(Debug)]
struct DisplayProbe {
    entered: mpsc::UnboundedSender<usize>,
    release_first: Arc<Semaphore>,
}

#[derive(Debug, Default)]
struct FakeRpc {
    calls: Mutex<Vec<&'static str>>,
    encoded: Mutex<Option<ExternalDocumentWire>>,
    display_probe: Option<DisplayProbe>,
    display_calls: AtomicUsize,
    fail_display: bool,
}

#[async_trait]
impl ExternalPackageRpc for FakeRpc {
    async fn frame(
        &self,
        _: &str,
        request: &ExternalFrameRequest,
    ) -> Result<ExternalFrameResult, ExternalPackageConnectionError> {
        Ok(ExternalFrameResult::Complete {
            consumed_bytes: request.bytes().unwrap().len(),
        })
    }
    async fn decode(
        &self,
        method: &str,
        _: &ExternalDecodeRequest,
    ) -> Result<ExternalDecodeResponse, ExternalPackageConnectionError> {
        assert_eq!(method, "hooks.upstream.decode");
        self.calls.lock().push("hooks.upstream.decode");
        Ok(ExternalDecodeResponse {
            document: serde_json::from_value(json!({"request": {"type":"string", "value":"sale"}}))
                .unwrap(),
        })
    }
    async fn encode(
        &self,
        method: &str,
        request: &ExternalEncodeRequest,
    ) -> Result<ExternalEncodeResponse, ExternalPackageConnectionError> {
        assert_eq!(method, "hooks.downstream.encode");
        self.calls.lock().push("hooks.downstream.encode");
        *self.encoded.lock() = Some(request.document.clone());
        Ok(ExternalEncodeResponse::from_bytes(b"approved"))
    }
    async fn display(
        &self,
        method: &str,
        _: &ExternalDisplayRequest,
    ) -> Result<ExternalDisplayResponse, ExternalPackageConnectionError> {
        if let Some(probe) = &self.display_probe {
            let display_number = self.display_calls.fetch_add(1, Ordering::SeqCst) + 1;
            let stable_method = match method {
                "document.upstream.display" => "document.upstream.display",
                "document.downstream.display" => "document.downstream.display",
                other => panic!("unexpected display method {other}"),
            };
            self.calls.lock().push(stable_method);
            probe.entered.send(display_number).unwrap();
            if display_number == 1 {
                probe.release_first.acquire().await.unwrap().forget();
            }
        }
        if self.fail_display {
            return Err(ExternalPackageConnectionError::Disconnected);
        }
        Ok(ExternalDisplayResponse {
            html: "ok".to_owned(),
        })
    }
}

fn listener_id() -> ListenerId {
    ListenerId::from_uuid(Uuid::from_u128(10))
}

fn registration() -> ExternalPackageRegistration {
    serde_json::from_value(json!({
        "api":1,"package":{"id":"external-local","name":"External Local","version":"1.0.0","description":"test"},
        "document":{
            "upstream":{"schema":{"id":"request","title":"Request","version":1,"fields":[{"name":"request","label":"Request","type":"string"}]},"display":"display"},
            "downstream":{"schema":{"id":"response","title":"Response","version":1,"fields":[{"name":"response_code","label":"Response code","type":"string"}]},"display":"display"}
        },
        "hooks":{
            "upstream":{"frame":"frame","decode":"decode","encode":"encode"},
            "downstream":{"frame":"frame","decode":"decode","encode":"encode"}
        }
    })).unwrap()
}

fn rules(registration: &ExternalPackageRegistration) -> ProtocolDocumentRuleConnectionFactory {
    let package = registration.package().identity().clone();
    let up = registration.document().upstream().schema().clone();
    let down = registration.document().downstream().schema().clone();
    let request_rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "request rule".to_owned(),
        true,
        10,
        1,
        listener_id(),
        package.clone(),
        up.version(),
        ProtocolRuleStage::AppToProxy,
        Vec::new(),
        vec![DocumentAction::RecordMatch],
    )
    .unwrap();
    let response_rule = ProtocolDocumentRuleDefinition::new_named_for_stage(
        ProtocolDocumentRuleId::new(),
        "response rule".to_owned(),
        true,
        10,
        2,
        listener_id(),
        package.clone(),
        down.version(),
        ProtocolRuleStage::ProxyToApp,
        Vec::new(),
        vec![DocumentAction::SetField {
            field: DocumentFieldName::new("response_code").unwrap(),
            value: DocumentValue::String("00".to_owned()),
        }],
    )
    .unwrap();
    let program = |stage, schema, rules| {
        Arc::new(
            ProtocolDocumentRuleProgram::new_for_stage(
                listener_id(),
                package.clone(),
                schema,
                stage,
                rules,
            )
            .unwrap(),
        )
    };
    ProtocolDocumentRuleConnectionFactory::new(
        program(
            ProtocolRuleStage::AppToProxy,
            up.clone(),
            vec![request_rule],
        ),
        program(ProtocolRuleStage::ProxyToUpstream, up, Vec::new()),
        program(ProtocolRuleStage::UpstreamToProxy, down.clone(), Vec::new()),
        program(ProtocolRuleStage::ProxyToApp, down, vec![response_rule]),
    )
    .unwrap()
}
