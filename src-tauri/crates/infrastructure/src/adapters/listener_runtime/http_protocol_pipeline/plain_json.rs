use std::{collections::BTreeMap, marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_domain::{Document, DocumentValue, JsonPointer, ProtocolDirection};
use intercept_proxy_exchange::{
    Decode, Direction, DirectionKind, Display, Error, ErrorStage, Http, HttpContext,
};
use intercept_proxy_runtime::{
    ConnectionContext, FaultAction, HttpDirectionCapabilities, HttpMessagePrepare,
    HttpRequestMetadata, Message, PipelinePorts,
};
use parking_lot::Mutex;

use super::{HttpDocumentRulePrograms, JointDocumentEvaluation};

pub(super) fn build_capabilities<D: Direction>(
    direction: ProtocolDirection,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
) -> HttpDirectionCapabilities<D> {
    let observed = Arc::new(Mutex::new(None));
    HttpDirectionCapabilities::prepared(
        Box::new(PlainDecode::<D> {
            direction,
            programs,
            listener_transaction,
            observed: Arc::clone(&observed),
            marker: PhantomData,
        }),
        Box::new(PlainDisplay {
            observed: Arc::clone(&observed),
        }),
        Box::new(PlainPrepare {
            direction,
            observed,
            marker: PhantomData,
        }),
    )
}

struct PlainObserved {
    original_document: Option<Document>,
    program: Arc<intercept_proxy_domain::UnifiedRuleProgram>,
    listener_transaction: tokio::sync::OwnedMutexGuard<()>,
}

struct PlainDecode<D: Direction> {
    direction: ProtocolDirection,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
    observed: Arc<Mutex<Option<PlainObserved>>>,
    marker: PhantomData<fn() -> D>,
}

#[async_trait]
impl<D: Direction> Decode<Http, D> for PlainDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        let listener_transaction = Arc::clone(&self.listener_transaction).lock_owned().await;
        let program = self.programs.read().program(self.direction);
        let original_document = if program.rules().is_empty() {
            None
        } else {
            if !context.body_is_utf8 {
                return Err(Error::coded(
                    "BODY_DECODE_FAILED",
                    "Plain HTTP JSON Body 必须是 UTF-8",
                ));
            }
            Some(
                Document::parse_json(&context.body)
                    .map_err(|error| Error::coded(error.code.as_str(), error.message))?,
            )
        };
        let document = original_document
            .clone()
            .unwrap_or_else(|| text_document(context));
        *self.observed.lock() = Some(PlainObserved {
            original_document,
            program,
            listener_transaction,
        });
        Ok(document)
    }
}

struct PlainDisplay {
    observed: Arc<Mutex<Option<PlainObserved>>>,
}

#[async_trait]
impl Display for PlainDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        if self
            .observed
            .lock()
            .as_ref()
            .is_some_and(|observed| observed.original_document.is_none())
        {
            return document_text(document, "body").cloned();
        }
        document
            .to_json()
            .map_err(|error| Error::coded(error.code.as_str(), error.message))
    }
}

struct PlainPrepare<D: Direction> {
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<PlainObserved>>>,
    marker: PhantomData<fn() -> D>,
}

#[async_trait]
impl<D: Direction> HttpMessagePrepare<D> for PlainPrepare<D> {
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
        let Some(original_document) = observed.original_document else {
            let _listener_transaction = observed.listener_transaction;
            return apply_policy::<D>(ports, connection, request, message, None).await;
        };
        let evaluation = JointDocumentEvaluation::new_plain_json(
            document.clone(),
            original_document,
            self.direction,
            [observed.program],
        )
        .with_listener_transaction(observed.listener_transaction);
        apply_policy::<D>(
            ports,
            connection,
            request,
            message,
            Some(Box::new(evaluation)),
        )
        .await
    }
}

pub(super) async fn apply_policy<D: Direction>(
    ports: &dyn PipelinePorts,
    connection: &ConnectionContext,
    request: &HttpRequestMetadata,
    message: &mut Message,
    joint: Option<Box<dyn intercept_proxy_runtime::HttpJointEvaluation>>,
) -> Result<Vec<FaultAction>, Error> {
    let result = match (D::KIND, joint) {
        (DirectionKind::Upstream, Some(joint)) => {
            ports
                .apply_request_joint_policy(connection, request, message, joint)
                .await
        }
        (DirectionKind::Downstream, Some(joint)) => {
            ports
                .apply_response_joint_policy(connection, request, message, joint)
                .await
        }
        (DirectionKind::Upstream, None) => {
            ports
                .apply_request_policy(connection, request, message)
                .await
        }
        (DirectionKind::Downstream, None) => {
            ports
                .apply_response_policy(connection, request, message)
                .await
        }
    };
    result.map_err(|error| proxy_error(error).at(ErrorStage::Rules, D::KIND))
}

fn text_document(context: &HttpContext) -> Document {
    let mut document = Document::new(DocumentValue::Object(BTreeMap::new()));
    document
        .set(
            &JsonPointer::property("header"),
            DocumentValue::String(context.header.clone()),
        )
        .expect("text document header path is valid");
    document
        .set(
            &JsonPointer::property("body"),
            DocumentValue::String(context.body.clone()),
        )
        .expect("text document body path is valid");
    document
}

fn document_text<'a>(document: &'a Document, field: &str) -> Result<&'a String, Error> {
    match document
        .resolve(&JsonPointer::property(field))
        .map_err(|error| Error::coded(error.code.as_str(), error.message))?
    {
        DocumentValue::String(value) => Ok(value),
        _ => Err(Error::coded(
            "HTTP_DOCUMENT_INVALID",
            format!("{field} must be a String"),
        )),
    }
}

fn proxy_error(error: intercept_proxy_runtime::ProxyError) -> Error {
    error.into_exchange_error()
}
