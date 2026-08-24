//! Rhai 协议包到 Socket Pipeline 五项 capability 的连接级适配。
//!
//! Rhai Engine 保留在单方向 blocking worker 中；Frame、Decode、Display、Encode 通过独立
//! command 调用同一 executor。Rules 是 Writer Pipeline 中的宿主阶段，不进入 Rhai command。

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolRuleStage};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Downstream, Encode, Error, Frame, FrameResult, Rules, Socket,
    SocketContext, Upstream,
};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolDirectionExecutor,
    ProtocolExecutionCancellation, ProtocolFrameInspector, ProtocolFramingLimits,
    ProtocolRuntimeLimits,
};
use intercept_proxy_runtime::{
    FrameBoundary, SocketConnectionIdentity, SocketDirectionCapabilities,
    SocketObservationMetadata, SocketProcessingFailure, SocketProcessingFailureKind,
    SocketProtocolCapabilityFactory,
};
use tokio::sync::{mpsc, oneshot};

use super::{
    ProtocolDocumentRuleConnection, ProtocolDocumentRuleConnectionFactory,
    scripted_snapshot::ScriptedSocketRuntimeSnapshot,
};
use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use limiter::{BlockingCommandSlots, acquire_command_permits};

mod failure;
pub(super) mod limiter;
mod limits;
use failure::{
    frame_boundary, framing_failure, invalid_limits, processing_failure, runtime_failure,
    worker_failure,
};
pub(super) use limits::pipeline_limits;
#[cfg(test)]
use limits::processing_budget_ms;

pub(super) struct ScriptedSocketCapabilityFactoryAdapter {
    package: RuntimeProtocolPackageSnapshot,
    upstream: DirectionExecutionPlan,
    downstream: DirectionExecutionPlan,
    rules: ProtocolDocumentRuleConnectionFactory,
    listener_id: String,
    runtime_limits: ProtocolRuntimeLimits,
    framing_limits: ProtocolFramingLimits,
    blocking_slots: BlockingCommandSlots,
    observation: SocketObservationMetadata,
}

impl ScriptedSocketCapabilityFactoryAdapter {
    pub(super) fn new(
        snapshot: &ScriptedSocketRuntimeSnapshot,
        listener_id: String,
        framing_limits: ProtocolFramingLimits,
        observation: SocketObservationMetadata,
    ) -> Self {
        Self {
            package: snapshot.package().clone(),
            upstream: snapshot.upstream(),
            downstream: snapshot.downstream(),
            rules: snapshot.rule_connections().clone(),
            listener_id,
            runtime_limits: snapshot.runtime_limits(),
            framing_limits,
            blocking_slots: BlockingCommandSlots::new_relay(snapshot.maximum_connections()),
            observation,
        }
    }

    fn build<D: Direction>(
        &self,
        connection: SocketConnectionIdentity,
        binding: DirectionBinding,
    ) -> Result<SocketDirectionCapabilities<D>, SocketProcessingFailure> {
        let context = format!("{}:{}", connection.runtime_epoch, connection.connection_id);
        let cancellation = ProtocolExecutionCancellation::new();
        let inspector = ProtocolFrameInspector::new_with_cancellation(
            self.package.compiled(),
            binding.protocol_direction,
            context.clone(),
            self.listener_id.clone(),
            self.runtime_limits,
            self.framing_limits,
            cancellation.clone(),
        );
        let executor = ProtocolDirectionExecutor::new_with_cancellation(
            self.package.compiled(),
            binding.plan,
            context,
            self.listener_id.clone(),
            self.runtime_limits,
            cancellation.clone(),
        )
        .map_err(|_| processing_failure("protocol direction executor construction failed"))?;
        let client = DirectionWorkerClient::spawn(
            DirectionWorkerState {
                inspector,
                executor,
            },
            self.blocking_slots.clone(),
            cancellation,
        );
        Ok(SocketDirectionCapabilities::new(
            Box::new(ScriptedFrame::<D>::new(client.clone())),
            Box::new(ScriptedDecode::<D>::new(client.clone())),
            Box::new(ScriptedDisplay::new(client.clone())),
            Box::new(OrderedRules::<D>::new(
                self.rules
                    .connection(connection.clone(), binding.first_rules),
                self.rules.connection(connection, binding.second_rules),
            )),
            Box::new(ScriptedEncode::<D>::new(client)),
        ))
    }
}

impl SocketProtocolCapabilityFactory for ScriptedSocketCapabilityFactoryAdapter {
    fn observation_metadata(&self) -> SocketObservationMetadata {
        self.observation.clone()
    }

    fn create_upstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Upstream>, SocketProcessingFailure> {
        self.build(
            connection,
            DirectionBinding {
                protocol_direction: ProtocolDirection::Upstream,
                first_rules: ProtocolRuleStage::AppToProxy,
                second_rules: ProtocolRuleStage::ProxyToUpstream,
                plan: self.upstream,
            },
        )
    }

    fn create_downstream(
        &self,
        connection: SocketConnectionIdentity,
    ) -> Result<SocketDirectionCapabilities<Downstream>, SocketProcessingFailure> {
        self.build(
            connection,
            DirectionBinding {
                protocol_direction: ProtocolDirection::Downstream,
                first_rules: ProtocolRuleStage::UpstreamToProxy,
                second_rules: ProtocolRuleStage::ProxyToApp,
                plan: self.downstream,
            },
        )
    }
}

#[derive(Clone, Copy)]
struct DirectionBinding {
    protocol_direction: ProtocolDirection,
    first_rules: ProtocolRuleStage,
    second_rules: ProtocolRuleStage,
    plan: DirectionExecutionPlan,
}

#[derive(Clone)]
struct DirectionWorkerClient {
    commands: mpsc::Sender<DirectionCommand>,
    cancellation: ProtocolExecutionCancellation,
}

impl DirectionWorkerClient {
    fn spawn(
        state: DirectionWorkerState,
        blocking_slots: BlockingCommandSlots,
        cancellation: ProtocolExecutionCancellation,
    ) -> Self {
        let (commands, receiver) = mpsc::channel(1);
        std::mem::drop(tokio::spawn(run_direction_worker(
            receiver,
            state,
            blocking_slots,
        )));
        Self {
            commands,
            cancellation,
        }
    }

    async fn send<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, SocketProcessingFailure>>) -> DirectionCommand,
    ) -> Result<T, SocketProcessingFailure> {
        let mut cancel_on_drop = CancelOnDrop::new(self.cancellation.clone());
        let (reply, receive) = oneshot::channel();
        self.commands
            .send(build(reply))
            .await
            .map_err(|_| worker_failure())?;
        let result = receive.await.map_err(|_| worker_failure())?;
        cancel_on_drop.disarm();
        result
    }
}

struct CancelOnDrop {
    cancellation: ProtocolExecutionCancellation,
    armed: bool,
}

impl CancelOnDrop {
    fn new(cancellation: ProtocolExecutionCancellation) -> Self {
        Self {
            cancellation,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel();
        }
    }
}

enum DirectionCommand {
    Frame {
        buffered: std::sync::Arc<[u8]>,
        reply: Reply<FrameBoundary>,
    },
    Decode {
        origin: Vec<u8>,
        reply: Reply<Document>,
    },
    Display {
        document: Document,
        reply: Reply<String>,
    },
    Encode {
        origin: Vec<u8>,
        document: Document,
        reply: Reply<Vec<u8>>,
    },
}

type Reply<T> = oneshot::Sender<Result<T, SocketProcessingFailure>>;

struct DirectionWorkerState {
    inspector: ProtocolFrameInspector,
    executor: ProtocolDirectionExecutor,
}

async fn run_direction_worker(
    mut commands: mpsc::Receiver<DirectionCommand>,
    mut state: DirectionWorkerState,
    blocking_slots: BlockingCommandSlots,
) {
    while let Some(mut command) = commands.recv().await {
        let Some(permits) = acquire_command_permits(&blocking_slots, &mut command).await else {
            continue;
        };
        match tokio::task::spawn_blocking(move || {
            let _permits = permits;
            run_command(state, command)
        })
        .await
        {
            Ok(next) => state = next,
            Err(_) => return,
        }
    }
}

fn run_command(mut state: DirectionWorkerState, command: DirectionCommand) -> DirectionWorkerState {
    match command {
        DirectionCommand::Frame { buffered, reply } => {
            let result = state
                .inspector
                .inspect_owned(buffered)
                .map(frame_boundary)
                .map_err(|error| framing_failure(&error));
            let _ = reply.send(result);
        }
        DirectionCommand::Decode { origin, reply } => {
            let result = state
                .executor
                .decode_document(&origin)
                .map_err(|error| runtime_failure(&error));
            let _ = reply.send(result);
        }
        DirectionCommand::Display { document, reply } => {
            let result = state
                .executor
                .display_document(&document)
                .map_err(|error| runtime_failure(&error));
            let _ = reply.send(result);
        }
        DirectionCommand::Encode {
            origin,
            document,
            reply,
        } => {
            let result = state
                .executor
                .encode_document(&origin, document)
                .map_err(|error| runtime_failure(&error));
            let _ = reply.send(result);
        }
    }
    state
}

struct ScriptedFrame<D: Direction> {
    client: DirectionWorkerClient,
    marker: std::marker::PhantomData<fn() -> D>,
}
impl<D: Direction> ScriptedFrame<D> {
    fn new(client: DirectionWorkerClient) -> Self {
        Self {
            client,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Frame<D> for ScriptedFrame<D> {
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error> {
        let boundary = self
            .client
            .send(|reply| DirectionCommand::Frame {
                buffered: std::sync::Arc::from(buffer),
                reply,
            })
            .await
            .map_err(|failure| stage_error::<D>(&failure))?;
        Ok(match boundary {
            FrameBoundary::NeedMore { .. } | FrameBoundary::NeedMoreUnknown => {
                FrameResult::NeedMore
            }
            FrameBoundary::Complete { bytes } => FrameResult::Complete { consumed: bytes },
            FrameBoundary::Reject { reason } => {
                return Err(Error::new(format!(
                    "{:?}|FRAME_REJECTED: {reason}",
                    D::KIND
                )));
            }
        })
    }
}

struct ScriptedDecode<D: Direction> {
    client: DirectionWorkerClient,
    marker: std::marker::PhantomData<fn() -> D>,
}
impl<D: Direction> ScriptedDecode<D> {
    fn new(client: DirectionWorkerClient) -> Self {
        Self {
            client,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Socket, D> for ScriptedDecode<D> {
    async fn decode(&mut self, context: &SocketContext) -> Result<Document, Error> {
        self.client
            .send(|reply| DirectionCommand::Decode {
                origin: context.data.clone(),
                reply,
            })
            .await
            .map_err(|failure| stage_error::<D>(&failure))
    }
}

struct ScriptedDisplay {
    client: DirectionWorkerClient,
}
impl ScriptedDisplay {
    fn new(client: DirectionWorkerClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl Display for ScriptedDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        self.client
            .send(|reply| DirectionCommand::Display {
                document: document.clone(),
                reply,
            })
            .await
            .map_err(|error| Error::new(format!("{}: Rhai Display failed", error.stable_code())))
    }
}

struct OrderedRules<D: Direction> {
    first: ProtocolDocumentRuleConnection,
    second: ProtocolDocumentRuleConnection,
    marker: std::marker::PhantomData<fn() -> D>,
}
impl<D: Direction> OrderedRules<D> {
    fn new(first: ProtocolDocumentRuleConnection, second: ProtocolDocumentRuleConnection) -> Self {
        Self {
            first,
            second,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Rules for OrderedRules<D> {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let first = self
            .first
            .execute(self.first.bind_document(document))
            .map_err(|_| {
                Error::new(format!(
                    "{:?}|RULE_FAILED: first Rules stage failed",
                    D::KIND
                ))
            })?;
        let second = self
            .second
            .execute(self.second.bind_document(first.into_parts().0))
            .map_err(|_| {
                Error::new(format!(
                    "{:?}|RULE_FAILED: second Rules stage failed",
                    D::KIND
                ))
            })?;
        Ok(second.into_parts().0)
    }
}

struct ScriptedEncode<D: Direction> {
    client: DirectionWorkerClient,
    marker: std::marker::PhantomData<fn() -> D>,
}
impl<D: Direction> ScriptedEncode<D> {
    fn new(client: DirectionWorkerClient) -> Self {
        Self {
            client,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Encode<Socket, D> for ScriptedEncode<D> {
    async fn encode(
        &mut self,
        original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        self.client
            .send(|reply| DirectionCommand::Encode {
                origin: original.data.clone(),
                document: document.clone(),
                reply,
            })
            .await
            .map(|data| SocketContext { data })
            .map_err(|failure| stage_error::<D>(&failure))
    }
}

fn stage_error<D: Direction>(failure: &SocketProcessingFailure) -> Error {
    let kind = match failure.kind {
        SocketProcessingFailureKind::ProcessingFailed => "PROCESSING_FAILED",
        _ => failure.stable_code(),
    };
    Error::new(format!("{:?}|{kind}: Rhai protocol stage failed", D::KIND))
}

#[cfg(test)]
mod tests;
