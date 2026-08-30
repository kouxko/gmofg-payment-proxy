//! 外部 Socket 数据面的启动端口与不可变绑定。

use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{
    Document, ProtocolDirection, ProtocolPackageRef, ProxyListener, ProxyWorkspace, SocketTopology,
};
use intercept_proxy_package_contract::{
    DecodeParams, DisplayParams, EncodeParams, FrameParams, FrameResult, PackageManifest,
};

use super::super::{ProtocolDocumentRuleConnectionFactory, scripted_snapshot};
use crate::adapters::{PackageTransportClient, PackageTransportError};

/// 外部连接的协议入口窄接口。
#[async_trait]
pub(crate) trait ExternalPackageRpc: fmt::Debug + Send + Sync {
    async fn frame(
        &self,
        direction: ProtocolDirection,
        request: FrameParams,
    ) -> Result<FrameResult, PackageTransportError>;
    async fn decode(
        &self,
        direction: ProtocolDirection,
        request: DecodeParams,
    ) -> Result<Document, PackageTransportError>;
    async fn encode(
        &self,
        direction: ProtocolDirection,
        request: EncodeParams,
    ) -> Result<String, PackageTransportError>;
    async fn display(
        &self,
        direction: ProtocolDirection,
        request: DisplayParams,
    ) -> Result<String, PackageTransportError>;
}

#[async_trait]
impl ExternalPackageRpc for PackageTransportClient {
    async fn frame(
        &self,
        direction: ProtocolDirection,
        request: FrameParams,
    ) -> Result<FrameResult, PackageTransportError> {
        match direction {
            ProtocolDirection::Upstream => self.upstream_frame(request).await,
            ProtocolDirection::Downstream => self.downstream_frame(request).await,
        }
    }
    async fn decode(
        &self,
        direction: ProtocolDirection,
        request: DecodeParams,
    ) -> Result<Document, PackageTransportError> {
        match direction {
            ProtocolDirection::Upstream => self.upstream_decode(request).await,
            ProtocolDirection::Downstream => self.downstream_decode(request).await,
        }
    }
    async fn encode(
        &self,
        direction: ProtocolDirection,
        request: EncodeParams,
    ) -> Result<String, PackageTransportError> {
        match direction {
            ProtocolDirection::Upstream => self.upstream_encode(request).await,
            ProtocolDirection::Downstream => self.downstream_encode(request).await,
        }
    }
    async fn display(
        &self,
        direction: ProtocolDirection,
        request: DisplayParams,
    ) -> Result<String, PackageTransportError> {
        match direction {
            ProtocolDirection::Upstream => self.upstream_display(request).await,
            ProtocolDirection::Downstream => self.downstream_display(request).await,
        }
    }
}

/// 注册快照与对应在线 actor 的不可分割绑定。
#[derive(Clone)]
pub(crate) struct ExternalSocketPackageBinding {
    pub(crate) registration: PackageManifest,
    pub(crate) rpc: Arc<dyn ExternalPackageRpc>,
    max_frame_bytes: usize,
}

impl ExternalSocketPackageBinding {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new(registration: PackageManifest, rpc: Arc<dyn ExternalPackageRpc>) -> Self {
        Self::with_limits(registration, rpc, 8 * 1024 * 1024)
    }

    pub(crate) fn with_limits(
        registration: PackageManifest,
        rpc: Arc<dyn ExternalPackageRpc>,
        max_frame_bytes: usize,
    ) -> Self {
        Self {
            registration,
            rpc,
            max_frame_bytes,
        }
    }
    pub(crate) const fn registration(&self) -> &PackageManifest {
        &self.registration
    }
    pub(crate) const fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }
}

impl fmt::Debug for ExternalSocketPackageBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalSocketPackageBinding")
            .field("package", &self.registration.package().identity())
            .finish_non_exhaustive()
    }
}

/// Listener 启动阶段解析外部协议包的最小端口。
#[async_trait]
pub(crate) trait ExternalSocketPackageProvider: fmt::Debug + Send + Sync {
    async fn resolve(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Option<ExternalSocketPackageBinding>>;
}

/// 一次 Listener start 冻结的外部注册合同与可热替换规则集合。
#[derive(Clone)]
pub(crate) struct ExternalSocketRuntimeSnapshot {
    pub(crate) binding: ExternalSocketPackageBinding,
    pub(crate) rules: ProtocolDocumentRuleConnectionFactory,
    topology: SocketTopology,
    rule_generation: Arc<AtomicU64>,
}

impl ExternalSocketRuntimeSnapshot {
    pub(crate) fn new(
        binding: ExternalSocketPackageBinding,
        rules: ProtocolDocumentRuleConnectionFactory,
        topology: SocketTopology,
    ) -> Self {
        Self {
            binding,
            rules,
            topology,
            rule_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 复用现有编译边界，原子替换运行中外部 Listener 的四阶段规则。
    pub(crate) async fn replace_document_rules(
        &self,
        adapter: &super::super::ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<()> {
        let generation = self.rule_generation.fetch_add(1, Ordering::AcqRel) + 1;
        let registration = self.binding.registration();
        let workspace = workspace.clone();
        let listener = listener.clone();
        let package = registration.package().identity().clone();
        let upstream_schema = registration
            .document()
            .upstream()
            .schema()
            .expect("validated Socket Manifest requires upstream schema")
            .clone();
        let downstream_schema = registration
            .document()
            .downstream()
            .schema()
            .expect("validated Socket Manifest requires downstream schema")
            .clone();
        let topology = self.topology.clone();
        let replacement = adapter
            .compile_document_rules_on_blocking_owner(move || {
                scripted_snapshot::compile_document_rules(
                    &workspace,
                    &listener,
                    &package,
                    &upstream_schema,
                    &downstream_schema,
                    &topology,
                )
            })
            .await?;
        self.publish_document_rules(generation, &replacement);
        Ok(())
    }

    fn publish_document_rules(
        &self,
        generation: u64,
        replacement: &ProtocolDocumentRuleConnectionFactory,
    ) {
        if self.rule_generation.load(Ordering::Acquire) == generation {
            self.rules.replace(replacement);
        }
    }
}

impl fmt::Debug for ExternalSocketRuntimeSnapshot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExternalSocketRuntimeSnapshot")
            .field("binding", &self.binding)
            .field("topology", &self.topology)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
