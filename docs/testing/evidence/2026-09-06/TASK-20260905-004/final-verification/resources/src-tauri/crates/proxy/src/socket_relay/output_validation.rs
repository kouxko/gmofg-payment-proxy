use std::{fmt::Debug, marker::PhantomData};

use async_trait::async_trait;
use intercept_proxy_exchange::{Direction, Error, ErrorStage, SocketContext};
use uuid::Uuid;

use super::{
    JointRuleConditionEvaluation, SocketJointEvaluation, SocketPipelineLimits,
    SocketProcessingFailureKind,
};

pub struct BoundedSocketJointEvaluation<D: Direction> {
    inner: Box<dyn SocketJointEvaluation>,
    max_output_bytes: usize,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> Debug for BoundedSocketJointEvaluation<D> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BoundedSocketJointEvaluation")
            .field("direction", &D::KIND)
            .field("max_output_bytes", &self.max_output_bytes)
            .finish_non_exhaustive()
    }
}

impl<D: Direction> BoundedSocketJointEvaluation<D> {
    pub fn new(inner: Box<dyn SocketJointEvaluation>, limits: SocketPipelineLimits) -> Self {
        Self {
            inner,
            max_output_bytes: limits.max_output_bytes(),
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> SocketJointEvaluation for BoundedSocketJointEvaluation<D> {
    fn gate(&mut self, rule_id: Uuid) -> crate::Result<JointRuleConditionEvaluation> {
        self.inner.gate(rule_id)
    }

    async fn encode(self: Box<Self>) -> Result<SocketContext, Error> {
        let context = self.inner.encode().await?;
        validate_socket_output::<D>(&context, self.max_output_bytes)?;
        Ok(context)
    }
}

pub(crate) fn validate_socket_output<D: Direction>(
    context: &SocketContext,
    max_output_bytes: usize,
) -> Result<(), Error> {
    if context.data.is_empty() {
        return Err(socket_encode_error::<D>(
            SocketProcessingFailureKind::EmptyOutput,
            "Encode returned an empty Socket context",
        ));
    }
    if context.data.len() > max_output_bytes {
        return Err(socket_encode_error::<D>(
            SocketProcessingFailureKind::OutputLimitExceeded,
            "Encode output exceeds the configured limit",
        ));
    }
    Ok(())
}

fn socket_encode_error<D: Direction>(
    kind: SocketProcessingFailureKind,
    message: impl Into<String>,
) -> Error {
    Error::coded(kind.as_str(), message)
        .with_stage(ErrorStage::Encode)
        .with_direction(D::KIND)
}
