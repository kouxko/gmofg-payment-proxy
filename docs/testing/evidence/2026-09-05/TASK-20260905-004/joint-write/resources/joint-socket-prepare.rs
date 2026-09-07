//! Socket Document rules and Encode share one write preparation transaction.

use std::{sync::Arc, time::SystemTime};

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_exchange::{Direction, Envelope, Error, PrepareWrite, Socket, SocketContext};
use intercept_proxy_runtime::{
    BoundedSocketJointEvaluation, ChannelId, ConnectionContext, PipelinePorts,
    SocketConnectionIdentity, SocketPayloadDirection, SocketPipelineLimits,
};
use parking_lot::Mutex;

use crate::adapters::ProtocolPackageRuntime;
use crate::adapters::listener_runtime::JointDocumentEvaluation;

#[derive(Clone)]
pub(super) struct ExternalSocketObserved {
    pub(super) document: Document,
    pub(super) input: Vec<u8>,
}

pub(super) struct JointSocketPrepare<D: Direction> {
    pipeline: Arc<dyn PipelinePorts>,
    connection: SocketConnectionIdentity,
    listener_id: String,
    runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    programs: super::DocumentProgramFactory,
    package: ProtocolPackageRef,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
    limits: SocketPipelineLimits,
    marker: std::marker::PhantomData<fn() -> D>,
}

impl<D: Direction> JointSocketPrepare<D> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        pipeline: Arc<dyn PipelinePorts>,
        connection: SocketConnectionIdentity,
        listener_id: String,
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
        programs: super::DocumentProgramFactory,
        package: ProtocolPackageRef,
        listener_transaction: Arc<tokio::sync::Mutex<()>>,
        limits: SocketPipelineLimits,
    ) -> Self {
        Self {
            pipeline,
            connection,
            listener_id,
            runtime,
            direction,
            observed,
            programs,
            package,
            listener_transaction,
            limits,
            marker: std::marker::PhantomData,
        }
    }

    async fn prepare_document(&mut self, document: &Document) -> Result<SocketContext, Error> {
        let observed = self.observed.lock().take().ok_or_else(|| {
            Error::coded(
                "SOCKET_DECODE_CONTEXT_MISSING",
                "write preparation requires the current decoded frame",
            )
        })?;
        let context = ConnectionContext {
            runtime_epoch: self.connection.runtime_epoch,
            connection_id: self.connection.connection_id,
            channel: ChannelId::new(self.listener_id.clone())
                .map_err(|error| Error::coded("CONFIG_INVALID", error.to_string()))?,
            peer_addr: self.connection.peer_addr,
            accepted_at: SystemTime::now(),
            tls_peer: None,
        };
        let payload_direction = match self.direction {
            ProtocolDirection::Upstream => SocketPayloadDirection::AppToUpstream,
            ProtocolDirection::Downstream => SocketPayloadDirection::UpstreamToApp,
        };
        let listener_transaction = Arc::clone(&self.listener_transaction).lock_owned().await;
        let direction_programs = self.programs.direction_programs(self.direction);
        let evaluation = JointDocumentEvaluation::new_external_socket(
            document.clone(),
            observed.document,
            observed.input,
            Arc::clone(&self.runtime),
            self.direction,
            self.package.clone(),
            direction_programs,
        )
        .with_listener_transaction(listener_transaction);
        self.pipeline
            .apply_socket_policy(
                &context,
                payload_direction,
                Box::new(BoundedSocketJointEvaluation::<D>::new(
                    Box::new(evaluation),
                    self.limits,
                )),
            )
            .await
            .map_err(intercept_proxy_runtime::ProxyError::into_exchange_error)
    }
}

#[async_trait]
impl<D: Direction> PrepareWrite<Socket, D> for JointSocketPrepare<D> {
    async fn prepare(&mut self, envelope: &Envelope<Socket, D>) -> Result<SocketContext, Error> {
        self.prepare_document(envelope.document()).await
    }
}

#[cfg(test)]
#[path = "joint_socket_tests.rs"]
mod tests;
