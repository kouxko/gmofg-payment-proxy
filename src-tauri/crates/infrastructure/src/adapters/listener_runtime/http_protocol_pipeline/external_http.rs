use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use intercept_proxy_domain::{BodyCodecKind, Document, ProtocolDirection};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Encode, Error, ExternalPackageCallFailure,
    ExternalPackageCallStage, Http, HttpContext, Rules,
};
use intercept_proxy_product_api::BodyCodec;
use intercept_proxy_runtime::{HttpConnectionIdentity, HttpDirectionCapabilities, Message};
use parking_lot::Mutex;

use crate::adapters::{
    PackageTransportError, ProtocolPackageRuntime, body_codecs::resolve_message_codec,
};

use super::super::external_relay::RuntimeExternalSocketPackageBinding;
use super::{HttpDocumentRulePrograms, JointDocumentEvaluation, JointHttpRuleRuntime};

#[cfg(test)]
pub(crate) fn decode_http_body_for_package(
    selected: BodyCodecKind,
    context: &HttpContext,
) -> Result<String, Error> {
    let codec = http_body_codec_for_package(selected, context)?;
    codec
        .decode(&context.wire_body)
        .map_err(|error| Error::new(format!("{}\n{}", error.code, error.message)))
}

fn http_body_codec_for_package(
    selected: BodyCodecKind,
    context: &HttpContext,
) -> Result<Arc<dyn BodyCodec>, Error> {
    let message = Message::from_raw_http1_head(
        context.header.as_bytes(),
        Bytes::copy_from_slice(&context.wire_body),
    )
    .map_err(|error| Error::new(format!("{}\n{}", error.code, error.message)))?;
    if message.headers.iter().any(|header| {
        header.name.eq_ignore_ascii_case(b"content-encoding")
            && !String::from_utf8_lossy(&header.value)
                .split(',')
                .all(|value| value.trim().eq_ignore_ascii_case("identity"))
    }) {
        return Err(Error::new(
            "HTTP_CONTENT_ENCODING_UNSUPPORTED\nHTTP 协议包只接受 identity Content-Encoding",
        ));
    }
    let codec = resolve_message_codec(selected, &message);
    if matches!(
        codec.id(),
        "raw" | "auto:raw" | "auto:missing" | "auto:unsupported"
    ) {
        return Err(Error::new(format!(
            "HTTP_BODY_CHARSET_UNSUPPORTED\nHTTP 协议包只接受明确的 UTF-8 或 Shift-JIS Body，当前 codec={}",
            codec.id()
        )));
    }
    Ok(codec)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_capabilities<D: Direction>(
    runtime: Arc<JointHttpRuleRuntime>,
    connection: &HttpConnectionIdentity,
    direction: ProtocolDirection,
    response: bool,
    codec: BodyCodecKind,
    binding: &RuntimeExternalSocketPackageBinding,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
) -> HttpDirectionCapabilities<D> {
    let observed = Arc::new(Mutex::new(None));
    let runtime_handle = binding.runtime();
    let package = binding.registration().package().identity().clone();
    let rules = ExternalHttpDocumentRules::new(
        runtime,
        connection.clone(),
        response,
        Arc::clone(&runtime_handle),
        direction,
        Arc::clone(&observed),
        programs,
        listener_transaction,
        package.clone(),
    );
    HttpDirectionCapabilities::new(
        Box::new(ExternalHttpDecode::<D>::new(
            Arc::clone(&runtime_handle),
            direction,
            codec,
            Arc::clone(&observed),
            package.clone(),
        )),
        Box::new(ExternalHttpDisplay::new(
            Arc::clone(&runtime_handle),
            direction,
            package,
        )),
        Box::new(rules),
        Box::new(HttpEncode::<D>::new()),
    )
}

struct ExternalHttpObserved {
    original_document: Document,
    original_input: String,
    codec: Arc<dyn BodyCodec>,
}

struct ExternalHttpDecode<D: Direction> {
    runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    codec: BodyCodecKind,
    observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
    package: intercept_proxy_domain::ProtocolPackageRef,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> ExternalHttpDecode<D> {
    fn new(
        runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        codec: BodyCodecKind,
        observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
        package: intercept_proxy_domain::ProtocolPackageRef,
    ) -> Self {
        Self {
            runtime,
            direction,
            codec,
            observed,
            package,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Http, D> for ExternalHttpDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        let codec = http_body_codec_for_package(self.codec, context)?;
        let input = codec
            .decode(&context.wire_body)
            .map_err(|error| Error::new(format!("{}\n{}", error.code, error.message)))?;
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

struct ExternalHttpDocumentRules {
    runtime: Arc<JointHttpRuleRuntime>,
    connection: HttpConnectionIdentity,
    response: bool,
    package_runtime: Arc<dyn ProtocolPackageRuntime>,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
    package: intercept_proxy_domain::ProtocolPackageRef,
}

impl ExternalHttpDocumentRules {
    #[allow(clippy::too_many_arguments)]
    fn new(
        runtime: Arc<JointHttpRuleRuntime>,
        connection: HttpConnectionIdentity,
        response: bool,
        package_runtime: Arc<dyn ProtocolPackageRuntime>,
        direction: ProtocolDirection,
        observed: Arc<Mutex<Option<ExternalHttpObserved>>>,
        programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
        listener_transaction: Arc<tokio::sync::Mutex<()>>,
        package: intercept_proxy_domain::ProtocolPackageRef,
    ) -> Self {
        Self {
            runtime,
            connection,
            response,
            package_runtime,
            direction,
            observed,
            programs,
            listener_transaction,
            package,
        }
    }
}

#[async_trait]
impl Rules for ExternalHttpDocumentRules {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let observed = self.observed.lock().take().ok_or_else(|| {
            Error::new("HTTP_PROTOCOL_CONTEXT_MISSING\nDocument 缺少对应 HTTP 上下文")
        })?;
        let listener_transaction = Arc::clone(&self.listener_transaction).lock_owned().await;
        let program = self.programs.read().program(self.direction);
        self.runtime.stage(
            self.connection.runtime_epoch,
            self.connection.connection_id,
            self.response,
            JointDocumentEvaluation::new_external(
                document.clone(),
                observed.original_document,
                observed.original_input,
                Arc::clone(&self.package_runtime),
                self.direction,
                observed.codec,
                self.package.clone(),
                [program],
            )
            .with_listener_transaction(listener_transaction),
        );
        Ok(document)
    }
}

fn external_rpc_error(
    package: intercept_proxy_domain::ProtocolPackageRef,
    direction: ProtocolDirection,
    stage: ExternalPackageCallStage,
    default_method: &'static str,
    error: &PackageTransportError,
) -> Error {
    let (method, request_id, remote_code, stable_code, remote_message, remote_data_summary) =
        match error {
            PackageTransportError::Remote {
                request_id,
                method,
                error,
            } => (
                (*method).to_owned(),
                Some(request_id.clone()),
                Some(error.code()),
                Some(error.data().code().as_str().to_owned()),
                Some(error.message().to_owned()),
                Some("object(fields=1)".to_owned()),
            ),
            PackageTransportError::Package { error } => (
                default_method.to_owned(),
                None,
                None,
                Some(error.code.as_str().to_owned()),
                Some(error.message.clone()),
                None,
            ),
            _ => (default_method.to_owned(), None, None, None, None, None),
        };
    Error::new(format!("EXTERNAL_PACKAGE_CALL_FAILED\n{error}")).with_external_package_call(
        ExternalPackageCallFailure {
            package,
            direction,
            stage,
            method,
            request_id,
            remote_code,
            stable_code,
            remote_message,
            remote_data_summary,
        },
    )
}

struct HttpEncode<D: Direction> {
    direction: PhantomData<fn() -> D>,
}

impl<D: Direction> HttpEncode<D> {
    fn new() -> Self {
        Self {
            direction: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Encode<Http, D> for HttpEncode<D> {
    async fn encode(
        &mut self,
        original: &HttpContext,
        _document: &Document,
    ) -> Result<HttpContext, Error> {
        Ok(original.clone())
    }
}
