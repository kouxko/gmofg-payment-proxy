//! 外部包四个 RPC 与宿主 Rules 的真实 Exchange capability 实现。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_exchange::{
    Decode, Direction, DirectionKind, Display, Document, Error, ErrorStage,
    ExternalPackageCallFailure, ExternalPackageCallStage as ExchangeExternalPackageCallStage,
    Frame, FrameResult, Socket, SocketContext,
};
use intercept_proxy_package_contract::FrameResult as PackageFrameResult;
use intercept_proxy_runtime::{SocketConnectionIdentity, SocketProcessingFailureKind};
use parking_lot::Mutex;

use super::joint_socket::ExternalSocketObserved;
use super::trace_external_rpc_failure;
use crate::adapters::{PackageTransportError, ProtocolPackageRuntime};

macro_rules! capability {
    ($name:ident $(<$d:ident>)?) => {
        pub(super) struct $name$(<$d: Direction>)? {
            runtime: Arc<dyn ProtocolPackageRuntime>,
            method: &'static str,
            package: ProtocolPackageRef,
            connection: SocketConnectionIdentity,
            direction: ProtocolDirection,
            $(marker: std::marker::PhantomData<fn() -> $d>,)?
        }
    };
}

capability!(ExternalFrame<D>);
capability!(ExternalDisplay);

pub(super) struct ExternalDecode<D: Direction> {
    runtime: Arc<dyn ProtocolPackageRuntime>,
    method: &'static str,
    package: ProtocolPackageRef,
    connection: SocketConnectionIdentity,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    marker: std::marker::PhantomData<fn() -> D>,
}

impl<D: Direction> ExternalFrame<D> {
    pub(super) fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            runtime,
            method,
            package,
            connection,
            direction,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Frame<D> for ExternalFrame<D> {
    async fn split(&mut self, buffer: &[u8]) -> Result<FrameResult, Error> {
        let result = self
            .runtime
            .frame(self.direction, buffer.to_vec())
            .await
            .map_err(|error| rpc_error(ExternalCallStage::Frame, self.method, &error, self))?;
        Ok(match result {
            PackageFrameResult::NeedMore { .. } => FrameResult::NeedMore,
            PackageFrameResult::Complete { consumed_bytes } => FrameResult::Complete {
                consumed: consumed_bytes.get(),
            },
            PackageFrameResult::Reject { reason } => {
                return Err(Error::coded(
                    SocketProcessingFailureKind::FrameRejected.as_str(),
                    format!("frame rejected: {reason}"),
                )
                .with_stage(ErrorStage::Frame)
                .with_direction(D::KIND));
            }
        })
    }
}

impl<D: Direction> ExternalDecode<D> {
    pub(super) fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
        observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    ) -> Self {
        Self {
            runtime,
            method,
            package,
            connection,
            direction,
            observed,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Socket, D> for ExternalDecode<D> {
    async fn decode(&mut self, context: &SocketContext) -> Result<Document, Error> {
        let response = self
            .runtime
            .decode_socket(self.direction, context.data.clone())
            .await
            .map_err(|error| rpc_error(ExternalCallStage::Decode, self.method, &error, self))?;
        *self.observed.lock() = Some(ExternalSocketObserved {
            document: response.clone(),
            input: context.data.clone(),
        });
        Ok(response)
    }
}

impl ExternalDisplay {
    pub(super) fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            runtime,
            method,
            package,
            connection,
            direction,
        }
    }
}

#[async_trait]
impl Display for ExternalDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        self.runtime
            .display(self.direction, document.clone())
            .await
            .map_err(|error| rpc_error(ExternalCallStage::Display, self.method, &error, self))
    }
}

trait DiagnosticContext {
    fn diagnostic(
        &self,
    ) -> (
        &ProtocolPackageRef,
        &SocketConnectionIdentity,
        ProtocolDirection,
    );
}

macro_rules! diagnostic {
    ($type:ident $(<$d:ident>)?) => {
        impl$(<$d: Direction>)? DiagnosticContext for $type$(<$d>)? {
            fn diagnostic(&self) -> (&ProtocolPackageRef, &SocketConnectionIdentity, ProtocolDirection) {
                (&self.package, &self.connection, self.direction)
            }
        }
    };
}
diagnostic!(ExternalFrame<D>);
diagnostic!(ExternalDecode<D>);
diagnostic!(ExternalDisplay);

#[derive(Clone, Copy)]
enum ExternalCallStage {
    Frame,
    Decode,
    Display,
}

impl ExternalCallStage {
    const fn diagnostic(self) -> ExternalPackageCallStage {
        match self {
            Self::Frame => ExternalPackageCallStage::Frame,
            Self::Decode => ExternalPackageCallStage::Decode,
            Self::Display => ExternalPackageCallStage::Display,
        }
    }
    const fn failure_kind(self) -> SocketProcessingFailureKind {
        match self {
            Self::Decode => SocketProcessingFailureKind::DecodeFailed,
            Self::Frame | Self::Display => SocketProcessingFailureKind::ProcessingFailed,
        }
    }

    const fn exchange_stage(self) -> ErrorStage {
        match self {
            Self::Frame => ErrorStage::Frame,
            Self::Decode => ErrorStage::Decode,
            Self::Display => ErrorStage::Display,
        }
    }
}

fn rpc_error(
    stage: ExternalCallStage,
    method: &str,
    error: &PackageTransportError,
    context: &impl DiagnosticContext,
) -> Error {
    let (package, connection, direction) = context.diagnostic();
    let diagnostic = trace_external_rpc_failure(
        package,
        connection,
        direction,
        stage.diagnostic(),
        method,
        error,
    );
    let kind = stage.failure_kind();
    let exchange_direction = match direction {
        ProtocolDirection::Upstream => DirectionKind::Upstream,
        ProtocolDirection::Downstream => DirectionKind::Downstream,
    };
    Error::coded(kind.as_str(), "external package RPC failed")
        .with_stage(stage.exchange_stage())
        .with_direction(exchange_direction)
        .with_external_package_call(ExternalPackageCallFailure {
            package: diagnostic.package,
            direction: diagnostic.direction,
            stage: match diagnostic.stage {
                ExternalPackageCallStage::Frame => ExchangeExternalPackageCallStage::Frame,
                ExternalPackageCallStage::Decode => ExchangeExternalPackageCallStage::Decode,
                ExternalPackageCallStage::Display => ExchangeExternalPackageCallStage::Display,
                ExternalPackageCallStage::Encode => ExchangeExternalPackageCallStage::Encode,
            },
            method: diagnostic.method,
            request_id: diagnostic.request_id,
            remote_code: diagnostic.remote_code,
            stable_code: diagnostic.stable_code,
            remote_message: diagnostic.remote_message,
            remote_data_summary: diagnostic.remote_data_summary,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use intercept_proxy_domain::{ProtocolPackageId, ProtocolPackageVersion};
    use intercept_proxy_exchange::{Downstream, Upstream};
    use uuid::Uuid;

    #[derive(Debug)]
    struct RejectingRuntime;

    #[async_trait]
    impl ProtocolPackageRuntime for RejectingRuntime {
        async fn frame(
            &self,
            _: ProtocolDirection,
            _: Vec<u8>,
        ) -> Result<PackageFrameResult, PackageTransportError> {
            Ok(PackageFrameResult::Reject {
                reason: "Upstream|WRITE_FAILED: misleading rejection text".into(),
            })
        }
    }

    async fn assert_frame_rejection<D: Direction>(direction: ProtocolDirection) {
        let mut frame = ExternalFrame::<D>::new(
            Arc::new(RejectingRuntime),
            "hooks.frame",
            ProtocolPackageRef {
                id: ProtocolPackageId::new("frame-reject-test").unwrap(),
                version: ProtocolPackageVersion::new("1.0.0").unwrap(),
            },
            SocketConnectionIdentity {
                runtime_epoch: Uuid::from_u128(1),
                connection_id: Uuid::from_u128(2),
                peer_addr: "127.0.0.1:12345".parse().unwrap(),
            },
            direction,
        );
        let error = frame.split(b"rejected frame").await.unwrap_err();
        assert_eq!(error.code.as_deref(), Some("FRAME_REJECTED"));
        assert_eq!(error.stage, Some(ErrorStage::Frame));
        assert_eq!(error.direction, Some(D::KIND));
    }

    #[tokio::test]
    async fn frame_rejection_preserves_code_stage_and_both_directions() {
        assert_frame_rejection::<Upstream>(ProtocolDirection::Upstream).await;
        assert_frame_rejection::<Downstream>(ProtocolDirection::Downstream).await;
    }
}
