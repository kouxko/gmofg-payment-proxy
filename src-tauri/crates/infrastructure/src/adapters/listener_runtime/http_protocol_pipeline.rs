//! HTTP 协议包到 Exchange 四项 capability 的生产装配。
//!
//! 本模块不再实现 `PipelinePorts`：通用 HTTP action/session 仍由原端口负责，而协议包的
//! Decode、Display、Rules、Encode 由 Exchange Pipeline 按固定顺序分别调用。每个 capability
//! 只进入一个 Rhai 单阶段 API，避免旧组合 processor 隐藏或重复执行阶段。

use std::{
    fmt,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    Document, HttpBodyProcessing, ProtocolDocumentRuleProgram, ProtocolRuleStage, ProxyListener,
    ProxyWorkspace,
};
use intercept_proxy_exchange::{
    Decode, Direction, Display, Downstream, Encode, Error, Http, HttpContext, Rules, Upstream,
};
use intercept_proxy_protocol_scripting::{
    DirectionExecutionPlan, ProtocolDirection, ProtocolDirectionExecutor, ProtocolPackageKind,
    ProtocolRuntimeError,
};
use intercept_proxy_runtime::{
    HttpConnectionIdentity, HttpDirectionCapabilities, HttpObservationMetadata,
    HttpProtocolCapabilityFactory,
};
use parking_lot::{Mutex, RwLock};

use crate::adapters::protocol_packages::runtime_snapshot::RuntimeProtocolPackageSnapshot;

use super::ListenerRuntimeAdapter;

mod joint_rules;
mod programs;
pub(crate) use joint_rules::{JointDocumentEvaluation, JointHttpRuleRuntime};
use programs::{HttpDocumentRulePrograms, compile_programs};

/// Listener 启动时冻结的协议包与规则集合。
///
/// 协议脚本版本在 Listener 生命周期内不可变；规则集合可原子替换。每次创建 Exchange 时会为
/// upstream/downstream 各创建一个独占执行器，Document 不会跨连接保存或复用。
#[derive(Clone)]
pub(super) struct HttpProtocolRuntimeSnapshot {
    package: RuntimeProtocolPackageSnapshot,
    programs: Arc<RwLock<HttpDocumentRulePrograms>>,
    rule_generation: Arc<AtomicU64>,
    metadata: HttpObservationMetadata,
    listener_id: intercept_proxy_domain::ListenerId,
    joint_rules: Arc<JointHttpRuleRuntime>,
}

impl fmt::Debug for HttpProtocolRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpProtocolRuntimeSnapshot")
            .field("package", self.package.compiled().package())
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

impl HttpProtocolRuntimeSnapshot {
    #[cfg(test)]
    pub(super) fn joint_runtime(&self) -> Arc<JointHttpRuleRuntime> {
        Arc::clone(&self.joint_rules)
    }

    #[cfg(test)]
    pub(super) fn take_joint_evaluation(
        &self,
        connection: &HttpConnectionIdentity,
        response: bool,
    ) -> Option<JointDocumentEvaluation> {
        self.joint_rules
            .take_identity(connection.runtime_epoch, connection.connection_id, response)
    }

    pub(super) async fn prepare_async(
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<Option<Arc<Self>>> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(None);
        };
        let HttpBodyProcessing::Protocol { package } = &http.body_processing else {
            return Ok(None);
        };
        let frozen = adapter
            .protocol_packages
            .freeze_for_listener_start_async(package)
            .await?;
        if frozen.compiled().kind() != ProtocolPackageKind::Http {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_KIND_MISMATCH",
                "HTTP Body 必须绑定 HTTP 协议包。",
            )
            .entity(listener.id.to_string()));
        }
        let workspace_for_compile = workspace.clone();
        let listener_for_compile = listener.clone();
        let package_for_compile = package.clone();
        let upstream_schema = frozen
            .compiled()
            .schema(ProtocolDirection::Upstream)
            .clone();
        let downstream_schema = frozen
            .compiled()
            .schema(ProtocolDirection::Downstream)
            .clone();
        let programs = adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_programs(
                    &workspace_for_compile,
                    &listener_for_compile,
                    &package_for_compile,
                    &upstream_schema,
                    &downstream_schema,
                )
            })
            .await?;
        Ok(Some(Arc::new(Self {
            package: frozen,
            programs: Arc::new(RwLock::new(programs)),
            rule_generation: Arc::new(AtomicU64::new(0)),
            metadata: HttpObservationMetadata {
                workspace_id: workspace.id.to_string(),
                listener_id: listener.id.to_string(),
            },
            listener_id: listener.id,
            joint_rules: Arc::clone(&adapter.joint_http_rules),
        })))
    }

    #[cfg(test)]
    pub(super) fn prepare(
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<Option<Arc<Self>>> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(None);
        };
        let HttpBodyProcessing::Protocol { package } = &http.body_processing else {
            return Ok(None);
        };
        let frozen = adapter
            .protocol_packages
            .freeze_for_listener_start(package)?;
        if frozen.compiled().kind() != ProtocolPackageKind::Http {
            return Err(AppError::new(
                "PROTOCOL_PACKAGE_KIND_MISMATCH",
                "HTTP Body 必须绑定 HTTP 协议包。",
            )
            .entity(listener.id.to_string()));
        }
        let programs = compile_programs(
            workspace,
            listener,
            package,
            frozen.compiled().schema(ProtocolDirection::Upstream),
            frozen.compiled().schema(ProtocolDirection::Downstream),
        )?;
        Ok(Some(Arc::new(Self {
            package: frozen,
            programs: Arc::new(RwLock::new(programs)),
            rule_generation: Arc::new(AtomicU64::new(0)),
            metadata: HttpObservationMetadata {
                workspace_id: workspace.id.to_string(),
                listener_id: listener.id.to_string(),
            },
            listener_id: listener.id,
            joint_rules: Arc::clone(&adapter.joint_http_rules),
        })))
    }

    pub(super) async fn replace_document_rules(
        &self,
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(());
        };
        let HttpBodyProcessing::Protocol { package } = &http.body_processing else {
            return Ok(());
        };
        let generation = self.rule_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let workspace = workspace.clone();
        let listener = listener.clone();
        let package = package.clone();
        let upstream_schema = self
            .package
            .compiled()
            .schema(ProtocolDirection::Upstream)
            .clone();
        let downstream_schema = self
            .package
            .compiled()
            .schema(ProtocolDirection::Downstream)
            .clone();
        let replacement = adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_programs(
                    &workspace,
                    &listener,
                    &package,
                    &upstream_schema,
                    &downstream_schema,
                )
            })
            .await?;
        if self.rule_generation.load(Ordering::Acquire) == generation {
            *self.programs.write() = replacement;
        }
        Ok(())
    }

    fn build<D: Direction>(
        &self,
        connection: &HttpConnectionIdentity,
        direction: ProtocolDirection,
        first: ProtocolRuleStage,
        second: ProtocolRuleStage,
        response: bool,
    ) -> Result<HttpDirectionCapabilities<D>, Error> {
        let executor = ProtocolDirectionExecutor::new(
            self.package.compiled(),
            DirectionExecutionPlan::new(direction),
            connection.connection_id.to_string(),
            self.listener_id.to_string(),
            self.package.runtime_limits(),
        )
        .map_err(|error| protocol_error(&error))?;
        let executor = Arc::new(Mutex::new(executor));
        let programs = self.programs.read();
        let observed = Arc::new(Mutex::new(None));
        let rules = HttpDocumentRules::new(
            Arc::clone(&self.joint_rules),
            connection.clone(),
            response,
            Arc::clone(&executor),
            Arc::clone(&observed),
            [programs.program(first), programs.program(second)],
        );
        Ok(HttpDirectionCapabilities::new(
            Box::new(HttpDecode::<D>::new(Arc::clone(&executor), observed)),
            Box::new(HttpDisplay::new(Arc::clone(&executor))),
            Box::new(rules),
            Box::new(HttpEncode::<D>::new()),
        ))
    }
}

impl HttpProtocolCapabilityFactory for HttpProtocolRuntimeSnapshot {
    fn observation_metadata(&self) -> HttpObservationMetadata {
        self.metadata.clone()
    }

    fn create_upstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Upstream>, Error> {
        self.build(
            &connection,
            ProtocolDirection::Upstream,
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
            false,
        )
    }

    fn create_downstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Downstream>, Error> {
        self.build(
            &connection,
            ProtocolDirection::Downstream,
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
            true,
        )
    }
}

pub(super) type SharedExecutor = Arc<Mutex<ProtocolDirectionExecutor>>;

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
        document: &Document,
    ) -> Result<HttpContext, Error> {
        let _ = document;
        Ok(original.clone())
    }
}

struct HttpDocumentRules {
    runtime: Arc<JointHttpRuleRuntime>,
    connection: HttpConnectionIdentity,
    response: bool,
    executor: SharedExecutor,
    observed: Arc<Mutex<Option<HttpContext>>>,
    programs: [Arc<ProtocolDocumentRuleProgram>; 2],
}

impl HttpDocumentRules {
    fn new(
        runtime: Arc<JointHttpRuleRuntime>,
        connection: HttpConnectionIdentity,
        response: bool,
        executor: SharedExecutor,
        observed: Arc<Mutex<Option<HttpContext>>>,
        programs: [Arc<ProtocolDocumentRuleProgram>; 2],
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

async fn run_stage<T: Send + 'static>(
    stage: impl FnOnce() -> Result<T, ProtocolRuntimeError> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(stage)
        .await
        .map_err(|_| Error::new("HTTP_PROTOCOL_WORKER_FAILED\nHTTP 协议处理任务异常终止"))?
        .map_err(|error| protocol_error(&error))
}

fn protocol_error(error: &ProtocolRuntimeError) -> Error {
    Error::new(format!("{}\n{error}", error.code()))
}
