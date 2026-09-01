//! 外部包四个 RPC 与宿主 Rules 的真实 Exchange capability 实现。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::{ProtocolDirection, ProtocolPackageRef};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Document, Error, ExternalPackageCallFailure,
    ExternalPackageCallStage as ExchangeExternalPackageCallStage, Frame, FrameResult, Socket,
    SocketContext,
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
            .map_err(|error| rpc_error::<D>(ExternalCallStage::Frame, self.method, &error, self))?;
        Ok(match result {
            PackageFrameResult::NeedMore { .. } => FrameResult::NeedMore,
            PackageFrameResult::Complete { consumed_bytes } => FrameResult::Complete {
                consumed: consumed_bytes.get(),
            },
            PackageFrameResult::Reject { reason } => {
                return Err(Error::new(format!("frame rejected: {reason}")));
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
            .map_err(|error| {
                rpc_error::<D>(ExternalCallStage::Decode, self.method, &error, self)
            })?;
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
            .map_err(|error| {
                rpc_error_untyped(ExternalCallStage::Display, self.method, &error, self)
            })
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
}

fn rpc_error<D: Direction>(
    stage: ExternalCallStage,
    method: &str,
    error: &PackageTransportError,
    context: &impl DiagnosticContext,
) -> Error {
    rpc_error_inner(
        stage,
        method,
        error,
        context,
        Some(format!("{:?}", D::KIND)),
    )
}

fn rpc_error_untyped(
    stage: ExternalCallStage,
    method: &str,
    error: &PackageTransportError,
    context: &impl DiagnosticContext,
) -> Error {
    rpc_error_inner(stage, method, error, context, None)
}

fn rpc_error_inner(
    stage: ExternalCallStage,
    method: &str,
    error: &PackageTransportError,
    context: &impl DiagnosticContext,
    direction_prefix: Option<String>,
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
    let prefix = direction_prefix.map_or_else(String::new, |value| format!("{value}|"));
    Error::new(format!(
        "{prefix}{}: external package RPC failed",
        kind.as_str()
    ))
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
