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
#[cfg(test)]
use intercept_proxy_exchange::{Encode, Rules};
#[cfg(test)]
use intercept_proxy_package_contract::EncodeParams;
use intercept_proxy_package_contract::{
    CanonicalBase64, DecodeParams, DisplayParams, FrameParams, FrameResult as PackageFrameResult,
};
use intercept_proxy_runtime::{SocketConnectionIdentity, SocketProcessingFailureKind};
use parking_lot::Mutex;

use super::joint_socket::ExternalSocketObserved;
use super::{ExternalPackageRpc, trace_external_rpc_failure};
use crate::adapters::PackageTransportError;
#[cfg(test)]
use crate::adapters::listener_runtime::ProtocolDocumentRuleConnection;

macro_rules! capability {
    ($name:ident $(<$d:ident>)?) => {
        pub(super) struct $name$(<$d: Direction>)? {
            rpc: Arc<dyn ExternalPackageRpc>,
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
#[cfg(test)]
capability!(ExternalEncode<D>);

pub(super) struct ExternalDecode<D: Direction> {
    rpc: Arc<dyn ExternalPackageRpc>,
    method: &'static str,
    package: ProtocolPackageRef,
    connection: SocketConnectionIdentity,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    marker: std::marker::PhantomData<fn() -> D>,
}

impl<D: Direction> ExternalFrame<D> {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            rpc,
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
            .rpc
            .frame(
                self.direction,
                FrameParams {
                    buffer: CanonicalBase64::from_bytes(buffer),
                },
            )
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
        rpc: Arc<dyn ExternalPackageRpc>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
        observed: Arc<Mutex<Option<ExternalSocketObserved>>>,
    ) -> Self {
        Self {
            rpc,
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
            .rpc
            .decode(
                self.direction,
                DecodeParams {
                    input: CanonicalBase64::from_bytes(&context.data)
                        .as_str()
                        .to_owned(),
                },
            )
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
        rpc: Arc<dyn ExternalPackageRpc>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            rpc,
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
        self.rpc
            .display(
                self.direction,
                DisplayParams {
                    document: document.clone(),
                },
            )
            .await
            .map_err(|error| {
                rpc_error_untyped(ExternalCallStage::Display, self.method, &error, self)
            })
    }
}

#[cfg(test)]
pub(super) struct OrderedRules<D: Direction> {
    first: ProtocolDocumentRuleConnection,
    second: ProtocolDocumentRuleConnection,
    marker: std::marker::PhantomData<fn() -> D>,
}

#[cfg(test)]
impl<D: Direction> OrderedRules<D> {
    pub(super) fn new(
        first: ProtocolDocumentRuleConnection,
        second: ProtocolDocumentRuleConnection,
    ) -> Self {
        Self {
            first,
            second,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
#[cfg(test)]
impl<D: Direction> Rules for OrderedRules<D> {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let first = self
            .first
            .execute(self.first.bind_document(document))
            .map_err(|_| stage_error::<D>(SocketProcessingFailureKind::RuleFailed))?;
        let second = self
            .second
            .execute(self.second.bind_document(first.into_parts().0))
            .map_err(|_| stage_error::<D>(SocketProcessingFailureKind::RuleFailed))?;
        Ok(second.into_parts().0)
    }
}

#[cfg(test)]
impl<D: Direction> ExternalEncode<D> {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: &'static str,
        package: ProtocolPackageRef,
        connection: SocketConnectionIdentity,
        direction: ProtocolDirection,
    ) -> Self {
        Self {
            rpc,
            method,
            package,
            connection,
            direction,
            marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
#[cfg(test)]
impl<D: Direction> Encode<Socket, D> for ExternalEncode<D> {
    async fn encode(
        &mut self,
        original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        let result = self
            .rpc
            .encode(
                self.direction,
                EncodeParams {
                    original_input: CanonicalBase64::from_bytes(&original.data)
                        .as_str()
                        .to_owned(),
                    document: document.clone(),
                },
            )
            .await
            .map_err(|error| {
                rpc_error::<D>(ExternalCallStage::Encode, self.method, &error, self)
            })?;
        Ok(SocketContext {
            data: result
                .try_into()
                .map(|value: CanonicalBase64| value.bytes())
                .map_err(|_| stage_error::<D>(SocketProcessingFailureKind::EncodeFailed))?,
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
#[cfg(test)]
diagnostic!(ExternalEncode<D>);

#[derive(Clone, Copy)]
enum ExternalCallStage {
    Frame,
    Decode,
    Display,
    #[cfg(test)]
    Encode,
}

impl ExternalCallStage {
    const fn diagnostic(self) -> ExternalPackageCallStage {
        match self {
            Self::Frame => ExternalPackageCallStage::Frame,
            Self::Decode => ExternalPackageCallStage::Decode,
            Self::Display => ExternalPackageCallStage::Display,
            #[cfg(test)]
            Self::Encode => ExternalPackageCallStage::Encode,
        }
    }
    const fn failure_kind(self) -> SocketProcessingFailureKind {
        match self {
            Self::Decode => SocketProcessingFailureKind::DecodeFailed,
            #[cfg(test)]
            Self::Encode => SocketProcessingFailureKind::EncodeFailed,
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

#[cfg(test)]
fn stage_error<D: Direction>(kind: SocketProcessingFailureKind) -> Error {
    Error::new(format!(
        "{:?}|{}: external package stage failed",
        D::KIND,
        kind.as_str()
    ))
}
