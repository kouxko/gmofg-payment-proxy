//! 外部 Socket 数据面的启动端口与不可变绑定。

use std::{fmt, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::AppResult;
use intercept_proxy_domain::{ProtocolPackageRef, ProxyListener, ProxyWorkspace, SocketTopology};
use intercept_proxy_package_contract::PackageManifest;

use super::super::{DocumentProgramFactory, document_rules};
use crate::adapters::ProtocolPackageRuntime;

/// 注册快照与对应在线 actor 的不可分割绑定。
#[derive(Clone)]
pub(crate) struct ExternalSocketPackageBinding {
    pub(crate) registration: PackageManifest,
    pub(crate) runtime: Arc<dyn ProtocolPackageRuntime>,
    max_frame_bytes: usize,
}

impl ExternalSocketPackageBinding {
    #[cfg(test)]
    #[must_use]
    pub(crate) fn new(
        registration: PackageManifest,
        runtime: Arc<dyn ProtocolPackageRuntime>,
    ) -> Self {
        Self::with_limits(registration, runtime, 8 * 1024 * 1024)
    }

    pub(crate) fn with_limits(
        registration: PackageManifest,
        runtime: Arc<dyn ProtocolPackageRuntime>,
        max_frame_bytes: usize,
    ) -> Self {
        Self {
            registration,
            runtime,
            max_frame_bytes,
        }
    }
    pub(crate) const fn registration(&self) -> &PackageManifest {
        &self.registration
    }
    pub(crate) const fn max_frame_bytes(&self) -> usize {
        self.max_frame_bytes
    }

    pub(crate) fn runtime(&self) -> Arc<dyn ProtocolPackageRuntime> {
        Arc::clone(&self.runtime)
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
    pub(crate) rules: DocumentProgramFactory,
    topology: SocketTopology,
    pub(crate) listener_transaction: Arc<tokio::sync::Mutex<()>>,
}

impl ExternalSocketRuntimeSnapshot {
    #[cfg(test)]
    pub(crate) fn new(
        binding: ExternalSocketPackageBinding,
        rules: DocumentProgramFactory,
        topology: SocketTopology,
    ) -> Self {
        Self::with_listener_transaction(
            binding,
            rules,
            topology,
            Arc::new(tokio::sync::Mutex::new(())),
        )
    }

    pub(crate) fn with_listener_transaction(
        binding: ExternalSocketPackageBinding,
        rules: DocumentProgramFactory,
        topology: SocketTopology,
        listener_transaction: Arc<tokio::sync::Mutex<()>>,
    ) -> Self {
        Self {
            binding,
            rules,
            topology,
            listener_transaction,
        }
    }

    pub(crate) async fn compile_replacement(
        &self,
        adapter: &super::super::ListenerRuntimeAdapter,
        workspace: &ProxyWorkspace,
        listener: &ProxyListener,
    ) -> AppResult<DocumentProgramFactory> {
        let registration = self.binding.registration();
        let workspace = workspace.clone();
        let listener = listener.clone();
        let package = registration.package().identity().clone();
        let upstream_schema = registration.document().upstream().schema().cloned();
        let downstream_schema = registration.document().downstream().schema().cloned();
        let topology = self.topology.clone();
        adapter
            .compile_document_rules_on_blocking_owner(move || {
                document_rules::compile_document_rules(
                    &workspace,
                    &listener,
                    &package,
                    upstream_schema.as_ref(),
                    downstream_schema.as_ref(),
                    &topology,
                )
            })
            .await
    }

    pub(crate) fn publish_replacement(&self, replacement: &DocumentProgramFactory) {
        self.rules.replace(replacement);
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
