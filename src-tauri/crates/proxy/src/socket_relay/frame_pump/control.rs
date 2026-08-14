use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex};

use tokio_util::sync::CancellationToken;

use crate::transport::relay::RelayBytes;

use super::super::processing::{
    LocalResponderProcessorFactory, ScriptedRelayProcessorFactory, SocketConnectionIdentity,
    SocketFrameProcessor, SocketPayloadDirection, SocketProcessingFailure,
    SocketProcessingFailureKind,
};

/// 运行一个方向；首个真实失败会取消 sibling，但 `Cancelled` 不会覆盖更具体的失败。
pub(super) async fn pump_and_cancel<F>(
    pump: F,
    cancellation: CancellationToken,
    first_failure: Arc<Mutex<Option<SocketProcessingFailure>>>,
) -> Result<(), SocketProcessingFailure>
where
    F: Future<Output = Result<(), SocketProcessingFailure>>,
{
    let result = pump.await;
    if let Err(failure) = &result {
        if failure.kind != SocketProcessingFailureKind::Cancelled {
            let mut recorded = first_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if recorded.is_none() {
                *recorded = Some(failure.clone());
            }
        }
        cancellation.cancel();
    }
    result
}

pub(super) fn create_relay_processor(
    factory: &dyn ScriptedRelayProcessorFactory,
    connection: SocketConnectionIdentity,
    direction: SocketPayloadDirection,
) -> Result<Box<dyn SocketFrameProcessor>, SocketProcessingFailure> {
    std::panic::catch_unwind(AssertUnwindSafe(|| {
        factory.create_direction(connection, direction)
    }))
    .map_err(|_| processor_factory_panic(direction, "scripted relay processor factory panicked"))
}

pub(super) fn create_local_processor(
    factory: &dyn LocalResponderProcessorFactory,
    connection: SocketConnectionIdentity,
) -> Result<Box<dyn SocketFrameProcessor>, SocketProcessingFailure> {
    std::panic::catch_unwind(AssertUnwindSafe(|| factory.create_exchange(connection))).map_err(
        |_| {
            processor_factory_panic(
                SocketPayloadDirection::LocalExchange,
                "local responder processor factory panicked",
            )
        },
    )
}

pub(super) fn choose_relay_result(
    first: Result<(), SocketProcessingFailure>,
    second: Result<(), SocketProcessingFailure>,
    bytes: RelayBytes,
) -> Result<RelayBytes, SocketProcessingFailure> {
    match (first, second) {
        (Ok(()), Ok(())) => Ok(bytes),
        (Err(first), Err(second)) if first.kind == SocketProcessingFailureKind::Cancelled => {
            Err(second)
        }
        (Err(first), _) => Err(first),
        (_, Err(second)) => Err(second),
    }
}

fn processor_factory_panic(
    direction: SocketPayloadDirection,
    message: &'static str,
) -> SocketProcessingFailure {
    SocketProcessingFailure::new(SocketProcessingFailureKind::ProcessorPanicked, message)
        .in_direction(direction)
}
