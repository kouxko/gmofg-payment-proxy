use std::{marker::PhantomData, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_domain::{Document, ProtocolDocumentRuleProgram};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Encode, Error, Http, HttpContext, Rules,
};
use intercept_proxy_protocol_scripting::{ProtocolDirectionExecutor, ProtocolRuntimeError};
use intercept_proxy_runtime::{HttpConnectionIdentity, HttpDirectionCapabilities};
use parking_lot::Mutex;

use super::{JointDocumentEvaluation, JointHttpRuleRuntime};

pub(crate) type SharedExecutor = Arc<Mutex<ProtocolDirectionExecutor>>;

pub(super) fn build_capabilities<D: Direction>(
    runtime: Arc<JointHttpRuleRuntime>,
    connection: &HttpConnectionIdentity,
    response: bool,
    executor: &SharedExecutor,
    programs: [Arc<ProtocolDocumentRuleProgram>; 1],
) -> HttpDirectionCapabilities<D> {
    let observed = Arc::new(Mutex::new(None));
    let rules = HttpDocumentRules::new(
        runtime,
        connection.clone(),
        response,
        Arc::clone(executor),
        Arc::clone(&observed),
        programs,
    );
    HttpDirectionCapabilities::new(
        Box::new(HttpDecode::<D>::new(Arc::clone(executor), observed)),
        Box::new(HttpDisplay::new(Arc::clone(executor))),
        Box::new(rules),
        Box::new(HttpEncode::<D>::new()),
    )
}

struct HttpDecode<D: Direction> {
    executor: SharedExecutor,
    observed: Arc<Mutex<Option<HttpContext>>>,
    direction: PhantomData<fn() -> D>,
}

impl<D: Direction> HttpDecode<D> {
    fn new(executor: SharedExecutor, observed: Arc<Mutex<Option<HttpContext>>>) -> Self {
        Self {
            executor,
            observed,
            direction: PhantomData,
        }
    }
}

#[async_trait]
impl<D: Direction> Decode<Http, D> for HttpDecode<D> {
    async fn decode(&mut self, context: &HttpContext) -> Result<Document, Error> {
        let executor = Arc::clone(&self.executor);
        let body = context.body.as_bytes().to_vec();
        let document = run_stage(move || executor.lock().decode_document(&body)).await?;
        *self.observed.lock() = Some(context.clone());
        Ok(document)
    }
}

struct HttpDisplay {
    executor: SharedExecutor,
}

impl HttpDisplay {
    fn new(executor: SharedExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl Display for HttpDisplay {
    async fn display(&mut self, document: &Document) -> Result<String, Error> {
        let executor = Arc::clone(&self.executor);
        let document = document.clone();
        run_stage(move || executor.lock().display_document(&document)).await
    }
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

struct HttpDocumentRules {
    runtime: Arc<JointHttpRuleRuntime>,
    connection: HttpConnectionIdentity,
    response: bool,
    executor: SharedExecutor,
    observed: Arc<Mutex<Option<HttpContext>>>,
    programs: [Arc<ProtocolDocumentRuleProgram>; 1],
}

impl HttpDocumentRules {
    fn new(
        runtime: Arc<JointHttpRuleRuntime>,
        connection: HttpConnectionIdentity,
        response: bool,
        executor: SharedExecutor,
        observed: Arc<Mutex<Option<HttpContext>>>,
        programs: [Arc<ProtocolDocumentRuleProgram>; 1],
    ) -> Self {
        Self {
            runtime,
            connection,
            response,
            executor,
            observed,
            programs,
        }
    }
}

#[async_trait]
impl Rules for HttpDocumentRules {
    async fn apply(&mut self, document: Document) -> Result<Document, Error> {
        let original = self.observed.lock().take().ok_or_else(|| {
            Error::new("HTTP_PROTOCOL_CONTEXT_MISSING\nDocument 缺少对应 HTTP 上下文")
        })?;
        self.runtime.stage(
            self.connection.runtime_epoch,
            self.connection.connection_id,
            self.response,
            JointDocumentEvaluation::new(
                document.clone(),
                original.body.into_bytes(),
                Arc::clone(&self.executor),
                self.programs.iter().cloned(),
            ),
        );
        Ok(document)
    }
}

pub(crate) async fn run_stage<T: Send + 'static>(
    stage: impl FnOnce() -> Result<T, ProtocolRuntimeError> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(stage)
        .await
        .map_err(|_| Error::new("HTTP_PROTOCOL_WORKER_FAILED\nHTTP 协议处理任务异常终止"))?
        .map_err(|error| protocol_error(&error))
}

pub(super) fn protocol_error(error: &ProtocolRuntimeError) -> Error {
    Error::new(format!("{}\n{error}", error.code()))
}
