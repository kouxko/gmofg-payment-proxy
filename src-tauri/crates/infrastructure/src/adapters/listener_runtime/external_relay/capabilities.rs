//! 外部包四个 RPC 与宿主 Rules 的真实 Exchange capability 实现。

use std::sync::Arc;

use async_trait::async_trait;
use intercept_proxy_application::ExternalPackageCallStage;
use intercept_proxy_domain::{
    ExternalDecodeRequest, ExternalDisplayRequest, ExternalDocumentWire, ExternalEncodeRequest,
    ExternalFrameRequest, ExternalFrameResult, ProtocolDirection, ProtocolPackageRef,
};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Document, Encode, Error, Frame, FrameResult, Rules, Socket,
    SocketContext,
};
use intercept_proxy_runtime::{SocketConnectionIdentity, SocketProcessingFailureKind};

use super::{ExternalPackageRpc, trace_external_rpc_failure};
use crate::adapters::{
    external_packages::ExternalPackageConnectionError,
    listener_runtime::ProtocolDocumentRuleConnection,
};

macro_rules! capability {
    ($name:ident $(<$d:ident>)?) => {
        pub(super) struct $name$(<$d: Direction>)? {
            rpc: Arc<dyn ExternalPackageRpc>,
            method: String,
            package: ProtocolPackageRef,
            connection: SocketConnectionIdentity,
            direction: ProtocolDirection,
            $(marker: std::marker::PhantomData<fn() -> $d>,)?
        }
    };
}

capability!(ExternalFrame<D>);
capability!(ExternalDecode<D>);
capability!(ExternalDisplay);
capability!(ExternalEncode<D>);

impl<D: Direction> ExternalFrame<D> {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: String,
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
            .frame(&self.method, &ExternalFrameRequest::from_bytes(buffer))
            .await
            .map_err(|error| {
                rpc_error::<D>(ExternalCallStage::Frame, &self.method, &error, self)
            })?;
        Ok(match result {
            ExternalFrameResult::NeedMore => FrameResult::NeedMore,
            ExternalFrameResult::Complete { consumed_bytes } => FrameResult::Complete {
                consumed: consumed_bytes,
            },
        })
    }
}

impl<D: Direction> ExternalDecode<D> {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: String,
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
impl<D: Direction> Decode<Socket, D> for ExternalDecode<D> {
    async fn decode(&mut self, context: &SocketContext) -> Result<Document, Error> {
        let response = self
            .rpc
            .decode(
                &self.method,
                &ExternalDecodeRequest::from_bytes(&context.data),
            )
            .await
            .map_err(|error| {
                rpc_error::<D>(ExternalCallStage::Decode, &self.method, &error, self)
            })?;
        Ok(response.document.into_document())
    }
}

impl ExternalDisplay {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: String,
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
                &self.method,
                &ExternalDisplayRequest {
                    document: ExternalDocumentWire::from_document(document),
                },
            )
            .await
            .map(|result| result.html)
            .map_err(|error| {
                rpc_error_untyped(ExternalCallStage::Display, &self.method, &error, self)
            })
    }
}

pub(super) struct OrderedRules<D: Direction> {
    first: ProtocolDocumentRuleConnection,
    second: ProtocolDocumentRuleConnection,
    marker: std::marker::PhantomData<fn() -> D>,
}

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

impl<D: Direction> ExternalEncode<D> {
    pub(super) fn new(
        rpc: Arc<dyn ExternalPackageRpc>,
        method: String,
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
impl<D: Direction> Encode<Socket, D> for ExternalEncode<D> {
    async fn encode(
        &mut self,
        _original: &SocketContext,
        document: &Document,
    ) -> Result<SocketContext, Error> {
        let result = self
            .rpc
            .encode(
                &self.method,
                &ExternalEncodeRequest {
                    document: ExternalDocumentWire::from_document(document),
                },
            )
            .await
            .map_err(|error| {
                rpc_error::<D>(ExternalCallStage::Encode, &self.method, &error, self)
            })?;
        Ok(SocketContext {
            data: result
                .bytes()
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
diagnostic!(ExternalEncode<D>);

#[derive(Clone, Copy)]
enum ExternalCallStage {
    Frame,
    Decode,
    Display,
    Encode,
}

impl ExternalCallStage {
    const fn diagnostic(self) -> ExternalPackageCallStage {
        match self {
            Self::Frame => ExternalPackageCallStage::Frame,
            Self::Decode => ExternalPackageCallStage::Decode,
            Self::Display => ExternalPackageCallStage::Display,
            Self::Encode => ExternalPackageCallStage::Encode,
        }
    }
    const fn failure_kind(self) -> SocketProcessingFailureKind {
        match self {
            Self::Decode => SocketProcessingFailureKind::DecodeFailed,
            Self::Encode => SocketProcessingFailureKind::EncodeFailed,
            Self::Frame | Self::Display => SocketProcessingFailureKind::ProcessingFailed,
        }
    }
}

fn rpc_error<D: Direction>(
    stage: ExternalCallStage,
    method: &str,
    error: &ExternalPackageConnectionError,
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
    error: &ExternalPackageConnectionError,
    context: &impl DiagnosticContext,
) -> Error {
    rpc_error_inner(stage, method, error, context, None)
}

fn rpc_error_inner(
    stage: ExternalCallStage,
    method: &str,
    error: &ExternalPackageConnectionError,
    context: &impl DiagnosticContext,
    direction_prefix: Option<String>,
) -> Error {
    let (package, connection, direction) = context.diagnostic();
    trace_external_rpc_failure(
        package,
        connection,
        direction,
        stage.diagnostic(),
        method,
        error,
    );
    let kind = if matches!(error, ExternalPackageConnectionError::Timeout { .. }) {
        SocketProcessingFailureKind::ProcessingTimeout
    } else {
        stage.failure_kind()
    };
    let prefix = direction_prefix.map_or_else(String::new, |value| format!("{value}|"));
    Error::new(format!(
        "{prefix}{}: external package RPC failed",
        kind.as_str()
    ))
}

fn stage_error<D: Direction>(kind: SocketProcessingFailureKind) -> Error {
    Error::new(format!(
        "{:?}|{}: external package stage failed",
        D::KIND,
        kind.as_str()
    ))
}
