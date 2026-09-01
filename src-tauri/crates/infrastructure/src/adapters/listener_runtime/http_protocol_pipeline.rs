//! HTTP 协议包到 Exchange 四项 capability 的生产装配。
//!
//! 本模块不再实现 `PipelinePorts`：通用 HTTP action/session 仍由原端口负责，而协议包的
//! Decode、Display、Rules、Encode 由 Exchange Pipeline 按固定顺序分别调用。每个 capability
//! 只进入协议包运行时的一个固定阶段方法，避免旧组合 processor 隐藏或重复执行阶段。

use std::{fmt, sync::Arc};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    BodyCodecKind, HttpBodyProcessing, ProtocolDirection, ProxyListener, ProxyWorkspace,
};
use intercept_proxy_exchange::{Direction, Downstream, Error, Upstream};
use intercept_proxy_package_contract::PackageKind;
use intercept_proxy_runtime::{
    HttpConnectionIdentity, HttpDirectionCapabilities, HttpObservationMetadata,
    HttpProtocolCapabilityFactory,
};
use parking_lot::RwLock;

use super::{ListenerRuntimeAdapter, external_relay::RuntimeExternalSocketPackageBinding};

mod external_http;
mod plain_json;
mod programs;
pub(super) use super::{JointDocumentEvaluation, JointHttpRuleRuntime};
#[cfg(test)]
pub(super) use external_http::decode_http_body_for_package;
use programs::{HttpDocumentRulePrograms, compile_programs};

/// Listener 启动时冻结的协议包与规则集合。
///
/// 精确协议包版本在 Listener 生命周期内不可变；规则集合可原子替换。upstream/downstream
/// capability 共享该版本对应的运行时实例，连接级 Document 只在 joint runtime 暂存。
#[derive(Clone)]
pub(super) struct HttpProtocolRuntimeSnapshot {
    external: Option<RuntimeExternalSocketPackageBinding>,
    request_codec: BodyCodecKind,
    response_codec: BodyCodecKind,
    programs: Arc<RwLock<HttpDocumentRulePrograms>>,
    metadata: HttpObservationMetadata,
    joint_rules: Arc<JointHttpRuleRuntime>,
    listener_transaction: Arc<tokio::sync::Mutex<()>>,
}

impl fmt::Debug for HttpProtocolRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpProtocolRuntimeSnapshot")
            .field(
                "package",
                &self.external.as_ref().map_or_else(
                    || None,
                    |binding| Some(format!("{:?}", binding.registration().package().identity())),
                ),
            )
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
    pub(super) fn rule_count(&self, direction: ProtocolDirection) -> usize {
        self.programs.read().program(direction).rules().len()
    }

    pub(super) async fn prepare_async(
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<Option<Arc<Self>>> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(None);
        };
        let (binding, upstream_schema, downstream_schema) = match &http.body_processing {
            HttpBodyProcessing::Plain => (None, None, None),
            HttpBodyProcessing::Protocol { package } => {
                let provider = adapter
                    .external_package_provider
                    .read()
                    .clone()
                    .ok_or_else(|| {
                        AppError::new(
                            "EXTERNAL_PACKAGE_PROVIDER_MISSING",
                            "HTTP 协议包注册表尚未装配。",
                        )
                        .entity(listener.id.to_string())
                    })?;
                let binding = provider.resolve(package).await?.ok_or_else(|| {
                    AppError::new(
                        "EXTERNAL_PACKAGE_NOT_FOUND",
                        "HTTP 协议包未在统一注册表中找到。",
                    )
                    .entity(format!("{}@{}", package.id, package.version))
                })?;
                if binding.registration().kind() != PackageKind::Http {
                    return Err(AppError::new(
                        "PROTOCOL_PACKAGE_KIND_MISMATCH",
                        "HTTP Body 必须绑定 HTTP 协议包。",
                    )
                    .entity(listener.id.to_string()));
                }
                let upstream_schema = binding
                    .registration()
                    .document()
                    .upstream()
                    .schema()
                    .cloned();
                let downstream_schema = binding
                    .registration()
                    .document()
                    .downstream()
                    .schema()
                    .cloned();
                (Some(binding), upstream_schema, downstream_schema)
            }
        };
        let owns_all_http = binding.is_some();
        let workspace_for_compile = workspace.clone();
        let listener_for_compile = listener.clone();
        let programs = adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_programs(
                    &workspace_for_compile,
                    &listener_for_compile,
                    upstream_schema.as_ref(),
                    downstream_schema.as_ref(),
                    owns_all_http,
                )
            })
            .await?;
        Ok(Some(Arc::new(Self {
            external: binding,
            request_codec: http.request_body_codec,
            response_codec: http.response_body_codec,
            programs: Arc::new(RwLock::new(programs)),
            metadata: HttpObservationMetadata {
                workspace_id: workspace.id.to_string(),
                listener_id: listener.id.to_string(),
            },
            joint_rules: Arc::clone(&adapter.joint_http_rules),
            listener_transaction: adapter.environment_apply_resource_gates.gate(
                &super::super::environment_configuration_lease::EnvironmentApplyLeaseResourceKey::Listener(
                    listener.id.as_uuid(),
                ),
            ),
        })))
    }

    pub(super) async fn compile_replacement(
        &self,
        adapter: &ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<HttpDocumentRulePrograms> {
        let intercept_proxy_domain::ListenerDataPlane::Http(http) = &listener.data_plane else {
            return Ok(self.programs.read().clone());
        };
        let owns_all_http = self.external.is_some();
        let workspace = workspace.clone();
        let listener = listener.clone();
        let (upstream_schema, downstream_schema) = match (&http.body_processing, &self.external) {
            (HttpBodyProcessing::Plain, None) => (None, None),
            (HttpBodyProcessing::Protocol { .. }, Some(binding)) => (
                binding
                    .registration()
                    .document()
                    .upstream()
                    .schema()
                    .cloned(),
                binding
                    .registration()
                    .document()
                    .downstream()
                    .schema()
                    .cloned(),
            ),
            _ => {
                return Err(AppError::new(
                    "HTTP_BODY_PROCESSING_RUNTIME_MISMATCH",
                    "HTTP Body 处理模式与运行快照不一致。",
                )
                .entity(listener.id.to_string()));
            }
        };
        adapter
            .compile_document_rules_on_blocking_owner(move || {
                compile_programs(
                    &workspace,
                    &listener,
                    upstream_schema.as_ref(),
                    downstream_schema.as_ref(),
                    owns_all_http,
                )
            })
            .await
    }

    pub(super) fn publish_replacement(&self, replacement: HttpDocumentRulePrograms) {
        *self.programs.write() = replacement;
    }

    fn build<D: Direction>(
        &self,
        connection: &HttpConnectionIdentity,
        direction: ProtocolDirection,
        response: bool,
    ) -> HttpDirectionCapabilities<D> {
        if let Some(binding) = &self.external {
            return external_http::build_capabilities(
                Arc::clone(&self.joint_rules),
                connection,
                direction,
                response,
                if response {
                    self.response_codec
                } else {
                    self.request_codec
                },
                binding,
                Arc::clone(&self.programs),
                Arc::clone(&self.listener_transaction),
            );
        }
        plain_json::build_capabilities(
            Arc::clone(&self.joint_rules),
            connection,
            direction,
            response,
            Arc::clone(&self.programs),
            Arc::clone(&self.listener_transaction),
        )
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
        if self.external.is_none() && !self.programs.read().has_rules(ProtocolDirection::Upstream) {
            return intercept_proxy_runtime::PlainHttpCapabilityFactory::new(
                self.metadata.workspace_id.clone(),
                self.metadata.listener_id.clone(),
            )
            .create_upstream(connection);
        }
        Ok(self.build(&connection, ProtocolDirection::Upstream, false))
    }

    fn create_downstream(
        &self,
        connection: HttpConnectionIdentity,
    ) -> Result<HttpDirectionCapabilities<Downstream>, Error> {
        if self.external.is_none()
            && !self
                .programs
                .read()
                .has_rules(ProtocolDirection::Downstream)
        {
            return intercept_proxy_runtime::PlainHttpCapabilityFactory::new(
                self.metadata.workspace_id.clone(),
                self.metadata.listener_id.clone(),
            )
            .create_downstream(connection);
        }
        Ok(self.build(&connection, ProtocolDirection::Downstream, true))
    }
}
