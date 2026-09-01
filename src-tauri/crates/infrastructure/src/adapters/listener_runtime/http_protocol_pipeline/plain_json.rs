use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolDirection};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Encode, Error, Http, HttpContext, Rules,
};
use intercept_proxy_runtime::{HttpConnectionIdentity, HttpDirectionCapabilities};
use parking_lot::Mutex;

use super::{HttpDocumentRulePrograms, JointDocumentEvaluation, JointHttpRuleRuntime};

pub(super) fn build_capabilities<D: Direction>(
    runtime: Arc<JointHttpRuleRuntime>,
    connection: &HttpConnectionIdentity,
    direction: ProtocolDirection,
    response: bool,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
) -> HttpDirectionCapabilities<D> {
    let observed = Arc::new(Mutex::new(None));
    HttpDirectionCapabilities::new(
        Box::new(PlainJsonDecode::<D>::new(Arc::clone(&observed))),
        Box::new(PlainJsonDisplay),
        Box::new(PlainJsonRules {
            runtime,
            connection: connection.clone(),
            response,
            direction,
            observed,
            programs,
            listener_transaction,
        }),
        Box::new(PreserveHttpContext::<D>(PhantomData)),
    )
}

struct PlainJsonDecode<D: Direction> {
    observed: Arc<Mutex<Option<Document>>>,
    marker: PhantomData<fn() -> D>,
}

impl<D: Direction> PlainJsonDecode<D> {
    fn new(observed: Arc<Mutex<Option<Document>>>) -> Self {
        Self {
            observed,
            marker: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Http, D> for PlainJsonDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        if !context.body_is_utf8 {
            return Err(Error::new(
                "BODY_DECODE_FAILED\nPlain HTTP JSON Body 必须是 UTF-8",
            ));
        }
        let document = Document::parse_json(&context.body)
            .map_err(|error| Error::new(format!("{}\n{}", error.code, error.message)))?;
        *self.observed.lock() = Some(document.clone());
        Ok(document)
    }
}

struct PlainJsonDisplay;

#[async_trait]
impl Display for PlainJsonDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        document
            .to_json()
            .map_err(|error| Error::new(format!("{}\n{}", error.code, error.message)))
    }
}

struct PlainJsonRules {
    runtime: Arc<JointHttpRuleRuntime>,
    connection: HttpConnectionIdentity,
    response: bool,
    direction: ProtocolDirection,
    observed: Arc<Mutex<Option<Document>>>,
    programs: Arc<parking_lot::RwLock<HttpDocumentRulePrograms>>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
}

#[async_trait]
impl Rules for PlainJsonRules {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let original_document = self.observed.lock().take().ok_or_else(|| {
            Error::new("HTTP_PROTOCOL_CONTEXT_MISSING\nDocument 缺少对应 HTTP 上下文")
        })?;
        let listener_transaction = Arc::clone(&self.listener_transaction).lock_owned().await;
        let program = self.programs.read().program(self.direction);
        self.runtime.stage(
            self.connection.runtime_epoch,
            self.connection.connection_id,
            self.response,
            JointDocumentEvaluation::new_plain_json(
                document.clone(),
                original_document,
                self.direction,
                [program],
            )
            .with_listener_transaction(listener_transaction),
        );
        Ok(document)
    }
}

struct PreserveHttpContext<D: Direction>(PhantomData<fn() -> D>);

#[async_trait]
impl<D: Direction> Encode<Http, D> for PreserveHttpContext<D> {
    async fn encode(
        &mut self,
        original: &HttpContext,
        _document: &Document,
    ) -> Result<HttpContext, Error> {
        Ok(original.clone())
    }
}
