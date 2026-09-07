use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_domain::{BodyCodecKind, Document, ProtocolDirection};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Error, ExternalPackageCallStage, Http, HttpContext,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{
    ConnectionContext, FaultAction, HttpDirectionCapabilities, HttpMessagePrepare,
    HttpRequestMetadata, Message, PipelinePorts,
};
use parking_lot::Mutex;

use crate::adapters::{ProtocolPackageRuntime, body_codecs::resolve_message_codec};

use super::super::external_relay::RuntimeExternalSocketPackageBinding;
use super::super::package_error::external_rpc_error;
use super::{HttpDocumentRulePrograms, JointDocumentEvaluation};

#[cfg(test)]
pub(crate) fn decode_http_body_for_package(
    selected: BodyCodecKind,
    context: &HttpContext,
) -> Result<String, Error> {
    let codec = http_body_codec_for_package(selected, context)?;
    codec
        .decode(&context.wire_body)
        .map_err(|error| Error::coded(error.code, error.message))
}

fn http_body_codec_for_package(
    selected: BodyCodecKind,
    context: &HttpContext,
) -> Result<Arc<dyn BodyCodec>, Error> {
    let message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .map_err(|error| Error::coded(error.code, error.message))?;
    if message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"content-encoding")
            && !String::from_utf8_lossy(&header.value)
                .split(',')
                .all(|value| value.trim().eq_ignore_ascii_case("identity"))
    }) {
        return Err(Error::coded(
            "HTTP_CONTENT_ENCODING_UNSUPPORTED",
            "HTTP 协议包只接受 identity Content-Encoding",
        ));
    }
    let codec = resolve_message_codec(selected, &message);
    if matches!(
        codec.id(),
        "raw" | "auto:raw" | "auto:missing" | "auto:unsupported"
    ) {
        return Err(Error::coded(
            "HTTP_BODY_CHARSET_UNSUPPORTED",
            format!(
                "HTTP 协议包只接受明确的 UTF-8 或 Shift-JIS Body，当前 codec={}",
                codec.id()
            ),
        ));
    }
    Ok(codec)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_capabilities<D: Direction>(
    direction: ProtocolDirection,
    codec: BodyCodecKind,
    binding: &RuntimeExternalSocketPackageBinding,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
) -> HttpDirectionCapabilities<D> {
    let observed = Arc::new(Mutex::new(None));
    let runtime_handle = binding.runtime();
    let package = binding.registration().package().identity().clone();
    HttpDirectionCapabilities::prepared(
        Box::new(ExternalHttpDecode::<D>::new(
            Arc::clone(&runtime_handle),
            direction,
            codec,
            Arc::clone(&observed),
            package.clone(),
            programs,
            listener_transaction,
        )),
        Box::new(ExternalHttpDisplay::new(
            Arc::clone(&runtime_handle),
            direction,
            package.clone(),
        )),
        Box::new(ExternalHttpPrepare::<D> {
            package_runtime: runtime_handle,
            direction,
            observed,
            package,
            marker: PhantomData,
        }),
    )
}

struct ExternalHttpObserved {
    original_document: Document,
    original_input: String,
    codec: Arc<dyn BodyCodec>,
    program: Arc<intercept_proxy_domain::UnifiedRuleProgram>,
    listener_transaction: tokio::sync::OwnedMutexGuard<()>,
}

struct ExternalHttpDecode<D: Direction> {
    runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    codec: BodyCodecKind,
    observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
    package: intercept_proxy_domain::ProtocolPackageRef,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> ExternalHttpDecode<D> {
    fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        codec: BodyCodecKind,
        observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
        package: intercept_proxy_domain::ProtocolPackageRef,
        programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
        listener_transaction: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            runtime,
            direction,
            codec,
            observed,
            package,
            programs,
            listener_transaction,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Http, D> for ExternalHttpDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        let listener_transaction = Arc::clone(&self.listener_transaction).lock_owned().await;
        let program = self.programs.read().program(self.direction);
        let codec = http_body_codec_for_package(self.codec, context)?;
        let input = codec
            .decode(&context.wire_body)
            .map_err(|error| Error::coded(error.code, error.message))?;
        let document = self
            .runtime
            .decode_http(self.direction, input.clone())
            .await
            .map_err(|error| {
                external_rpc_error(
                    self.package.clone(),
                    self.direction,
                    ExternalPackageCallStage::Decode,
                    "hooks.decode",
                    &error,
                )
            })?;
        *self.observed.lock() = Some(ExternalHttpObserved {
            original_document: document.clone(),
            original_input: input,
            codec,
            program,
            listener_transaction,
        });
        Ok(document)
    }
}

struct ExternalHttpDisplay {
    runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    package: intercept_proxy_domain::ProtocolPackageRef,
}

impl ExternalHttpDisplay {
    fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        package: intercept_proxy_domain::ProtocolPackageRef,
    ) -> Self {
        Self {
            runtime,
            direction,
            package,
        }
    }
}

#[async_trait]
impl Display for ExternalHttpDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        self.runtime
            .display(self.direction, document.clone())
            .await
            .map_err(|error| {
                external_rpc_error(
                    self.package.clone(),
                    self.direction,
                    ExternalPackageCallStage::Display,
                    "hooks.display",
                    &error,
                )
            })
    }
}

struct ExternalHttpPrepare<D: Direction> {
    package_runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
    package: intercept_proxy_domain::ProtocolPackageRef,
    marker: PhantomData<fn() -> D>,
}

#[async_trait]
impl<D: Direction> HttpMessagePrepare<D> for ExternalHttpPrepare<D> {
    async fn prepare(
        &mut self,
        connection: &ConnectionContext,
        request: &HttpRequestMetadata,
        message: &mut Message,
        _original: &HttpContext,
        document: &Document,
        ports: &dyn PipelinePorts,
    ) -> Result<Vec<FaultAction>, Error> {
        let observed = self.observed.lock().take().ok_or_else(|| {
            Error::coded(
                "HTTP_PROTOCOL_CONTEXT_MISSING",
                "Document 缺少对应 HTTP 上下文",
            )
        })?;
        let evaluation = JointDocumentEvaluation::new_external(
            document.clone(),
            observed.original_document,
            observed.original_input,
            Arc::clone(&self.package_runtime),
            self.direction,
            observed.codec,
            self.package.clone(),
            [observed.program],
        )
        .with_listener_transaction(observed.listener_transaction);
        super::plain_json::apply_policy::<D>(
            ports,
            connection,
            request,
            message,
            Some(Box::new(evaluation)),
        )
        .await
    }
}
