//! Socket Document rules and Encode share one actor-owned transaction.

use std::{sync::Arc, time::SystemTime};

use async_trait::async_trait;
use intercept_proxy_domain::{
    Document, ProtocolDirection, ProtocolDocumentRuleProgram, ProtocolPackageRef,
};
use intercept_proxy_exchange::{Direction, Encode, Error, Rules, Socket, SocketContext};
use intercept_proxy_runtime::{
    ChannelId, ConnectionContext, PipelinePorts, SocketConnectionIdentity, SocketPayloadDirection,
    SocketProcessingFailureKind,
};
use parking_lot::Mutex;

use super::ExternalPackageRpc;
use crate::adapters::listener_runtime::JointDocumentEvaluation;

#[derive(Clone)]
pub(super) struct ExternalSocketObserved {
    pub(super) document: Document,
    pub(super) input: Vec<u8>,
}

pub(super) struct JointSocketRules {
    pipeline: Arc<dyn PipelinePorts>,
    connection: SocketConnectionIdentity,
    listener_id: String,
    rpc: Arc<dyn ExternalPackageRpc>,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    prepared: Arc<Mutex<Option<SocketContext>>>,
    programs: [Arc<ProtocolDocumentRuleProgram>; 1],
    package: ProtocolPackageRef,
}

impl JointSocketRules {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        pipeline: Arc<dyn PipelinePorts>,
        connection: SocketConnectionIdentity,
        listener_id: String,
        rpc: Arc<dyn ExternalPackageRpc>,
        direction: ProtocolDirection,
        observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
        prepared: Arc<Mutex<Option<SocketContext>>>,
        programs: [Arc<ProtocolDocumentRuleProgram>; 1],
        package: ProtocolPackageRef,
    ) -> Self {
        Self {
            pipeline,
            connection,
            listener_id,
            rpc,
            direction,
            observed,
            prepared,
            programs,
            package,
        }
    }
}

#[async_trait]
impl Rules for JointSocketRules {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let observed = self.observed.lock().take().ok_or_else(|| {
            Error::new("SOCKET_DECODE_CONTEXT_MISSING: Rules requires the current decoded frame")
        })?;
        let context = ConnectionContext {
            runtime_epoch: self.connection.runtime_epoch,
            connection_id: self.connection.connection_id,
            channel: ChannelId::new(self.listener_id.clone())
                .map_err(|error| Error::new(error.to_string()))?,
            peer_addr: self.connection.peer_addr,
            accepted_at: SystemTime::now(),
            tls_peer: None,
        };
        let payload_direction = match self.direction {
            ProtocolDirection::Upstream => SocketPayloadDirection::AppToUpstream,
            ProtocolDirection::Downstream => SocketPayloadDirection::UpstreamToApp,
        };
        let evaluation = JointDocumentEvaluation::new_external_socket(
            document.clone(),
            observed.document,
            observed.input,
            Arc::clone(&self.rpc),
            self.direction,
            self.package.clone(),
            self.programs.iter().cloned(),
        );
        let encoded = self
            .pipeline
            .apply_socket_policy(&context, payload_direction, Box::new(evaluation))
            .await
            .map_err(|error| {
                let mut mapped = Error::new(error.message);
                mapped.external_package_call = error.external_package_call;
                mapped
            })?;
        *self.prepared.lock() = Some(encoded);
        Ok(document)
    }
}

pub(super) struct PreparedSocketEncode<D: Direction> {
    prepared: Arc<Mutex<Option<SocketContext>>>,
    marker: std::marker::PhantomData<fn() -> D>,
}

impl<D: Direction> PreparedSocketEncode<D> {
    pub(super) fn new(prepared: Arc<Mutex<Option<SocketContext>>>) -> Self {
        Self {
            prepared,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Encode<Socket, D> for PreparedSocketEncode<D> {
    async fn encode(
        &mut self,
        _original: &SocketContext,
        _document: &Document,
    ) -> Result<SocketContext, Error> {
        self.prepared.lock().take().ok_or_else(|| {
            Error::new(format!(
                "{:?}|{}: joint Socket Encode output is missing",
                D::KIND,
                SocketProcessingFailureKind::EncodeFailed.as_str(),
            ))
        })
    }
}
