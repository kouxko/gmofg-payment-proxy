//! HTTP 协议包到 Exchange 四项 capability 的生产装配。
//!
//! 本模块不再实现 `PipelinePorts`：通用 HTTP action/session 仍由原端口负责，而协议包的
//! Decode、Display、Rules、Encode 由 Exchange Pipeline 按固定顺序分别调用。每个 capability
//! 只进入 Sidecar JSON-RPC 的一个固定阶段方法，避免旧组合 processor 隐藏或重复执行阶段。

use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use intercept_proxy_application::{AppError, AppResult};
use intercept_proxy_domain::{
    BodyCodecKind, DocumentSchemaNode, HttpBodyProcessing, ProtocolDirection, ProtocolRuleStage,
    ProxyListener, ProxyWorkspace,
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
mod programs;
pub(super) use super::{JointDocumentEvaluation, JointHttpRuleRuntime};
#[cfg(test)]
pub(super) use external_http::decode_http_body_for_package;
use programs::{HttpDocumentRulePrograms, compile_programs};

/// Listener 启动时冻结的协议包与规则集合。
///
/// 精确协议包版本在 Listener 生命周期内不可变；规则集合可原子替换。upstream/downstream
/// capability 共享该版本对应的在线 Sidecar actor，连接级 Document 只在 joint runtime 暂存。
#[derive(Clone)]
pub(super) struct HttpProtocolRuntimeSnapshot {
    external: Option<RuntimeExternalSocketPackageBinding>,
    request_codec: BodyCodecKind,
    response_codec: BodyCodecKind,
    programs: Arc<RwLock<HttpDocumentRulePrograms>>,
    rule_generation: Arc<AtomicU64>,
    metadata: HttpObservationMetadata,
    joint_rules: Arc<JointHttpRuleRuntime>,
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
        let workspace_for_compile = workspace.clone();
        let listener_for_compile = listener.clone();
        let package_for_compile = package.clone();
        let fallback_schema = DocumentSchemaNode::Object {
            title: None,
            properties: BTreeMap::new(),
        };
        let upstream_schema = binding
            .registration()
            .document()
            .upstream()
            .schema()
            .unwrap_or(&fallback_schema)
            .clone();
        let downstream_schema = binding
            .registration()
            .document()
            .downstream()
            .schema()
            .unwrap_or(&fallback_schema)
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
            external: Some(binding),
            request_codec: http.request_body_codec,
            response_codec: http.response_body_codec,
            programs: Arc::new(RwLock::new(programs)),
            rule_generation: Arc::new(AtomicU64::new(0)),
            metadata: HttpObservationMetadata {
                workspace_id: workspace.id.to_string(),
                listener_id: listener.id.to_string(),
            },
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
        let fallback_schema = DocumentSchemaNode::Object {
            title: None,
            properties: BTreeMap::new(),
        };
        let Some(binding) = &self.external else {
            return Err(AppError::new(
                "EXTERNAL_PACKAGE_NOT_FOUND",
                "HTTP 协议包统一注册绑定不存在。",
            )
            .entity(listener.id.to_string()));
        };
        let (upstream_schema, downstream_schema) = (
            binding
                .registration()
                .document()
                .upstream()
                .schema()
                .unwrap_or(&fallback_schema)
                .clone(),
            binding
                .registration()
                .document()
                .downstream()
                .schema()
                .unwrap_or(&fallback_schema)
                .clone(),
        );
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
        stage: ProtocolRuleStage,
        response: bool,
    ) -> Result<HttpDirectionCapabilities<D>, Error> {
        if let Some(binding) = &self.external {
            let programs = self.programs.read();
            return Ok(external_http::build_capabilities(
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
                [programs.program(stage)],
            ));
        }
        Err(Error::new(
            "EXTERNAL_PACKAGE_NOT_FOUND\nHTTP 协议包统一注册绑定不存在",
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
            ProtocolRuleStage::ProxyToApp,
            true,
        )
    }
}
