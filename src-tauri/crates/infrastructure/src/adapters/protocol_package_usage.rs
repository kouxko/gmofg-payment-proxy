//! Workspace 与 Listener 运行态到协议包引用查询端口的适配器。
//!
//! 查询始终遍历全部 Workspace，并且只用完整 [`ProtocolPackageRef`] 相等判断。协议包 ID
//! 相同但版本不同、版本文本相同但 ID 不同，都不会被误判为当前版本的使用者。

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppError, AppResult, ListenerRuntimePort, ListenerRuntimeState, ProtocolPackageRef,
    ProtocolPackageUsageCount, ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel,
    WorkspaceRepositoryPort,
};
use intercept_proxy_domain::{ListenerDataPlane, SocketPayloadProcessing};

#[derive(Debug)]
pub struct ProtocolPackageUsageQueryAdapter {
    workspaces: Arc<dyn WorkspaceRepositoryPort>,
    listener_runtime: Arc<dyn ListenerRuntimePort>,
}

impl ProtocolPackageUsageQueryAdapter {
    #[must_use]
    pub fn new(
        workspaces: Arc<dyn WorkspaceRepositoryPort>,
        listener_runtime: Arc<dyn ListenerRuntimePort>,
    ) -> Self {
        Self {
            workspaces,
            listener_runtime,
        }
    }

    async fn all_usages(
        &self,
    ) -> AppResult<Vec<(ProtocolPackageRef, ProtocolPackageUsageViewModel)>> {
        let runtime_states = self
            .listener_runtime
            .statuses()
            .await?
            .into_iter()
            .map(|status| (status.listener_id, status.state))
            .collect::<BTreeMap<_, _>>();
        let mut usages = Vec::new();
        for summary in self.workspaces.list().await? {
            let workspace = self.workspaces.get(summary.id).await?;
            for listener in &workspace.listeners {
                let ListenerDataPlane::Socket(socket) = &listener.data_plane else {
                    continue;
                };
                let SocketPayloadProcessing::Scripted(scripted) = &socket.processing else {
                    continue;
                };
                usages.push((
                    scripted.package.clone(),
                    ProtocolPackageUsageViewModel {
                        workspace_id: workspace.id,
                        workspace_name: workspace.name.clone(),
                        listener_id: listener.id,
                        listener_name: listener.name.clone(),
                        listener_enabled: listener.enabled,
                        runtime_state: runtime_states
                            .get(&listener.id)
                            .cloned()
                            .unwrap_or(ListenerRuntimeState::Stopped),
                    },
                ));
            }
        }
        usages.sort_by(|left, right| {
            left.0
                .id
                .cmp(&right.0.id)
                .then_with(|| left.0.version.cmp(&right.0.version))
                .then_with(|| left.1.workspace_id.cmp(&right.1.workspace_id))
                .then_with(|| left.1.listener_id.cmp(&right.1.listener_id))
        });
        Ok(usages)
    }
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for ProtocolPackageUsageQueryAdapter {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
        Ok(self
            .all_usages()
            .await?
            .into_iter()
            .filter_map(|(candidate, usage)| (candidate == *package).then_some(usage))
            .collect())
    }

    async fn usage_counts(&self) -> AppResult<Vec<ProtocolPackageUsageCount>> {
        let mut counts = HashMap::<ProtocolPackageRef, (usize, usize)>::new();
        for (package, usage) in self.all_usages().await? {
            let count = counts.entry(package).or_default();
            count.0 = count.0.checked_add(1).ok_or_else(usage_count_error)?;
            if usage.blocks_disable() {
                count.1 = count.1.checked_add(1).ok_or_else(usage_count_error)?;
            }
        }
        let mut counts = counts
            .into_iter()
            .map(
                |(package, (reference_count, active_reference_count))| ProtocolPackageUsageCount {
                    package,
                    reference_count,
                    active_reference_count,
                },
            )
            .collect::<Vec<_>>();
        counts.sort_by(|left, right| {
            left.package
                .id
                .cmp(&right.package.id)
                .then_with(|| left.package.version.cmp(&right.package.version))
        });
        Ok(counts)
    }
}

fn usage_count_error() -> AppError {
    AppError::new(
        "PROTOCOL_PACKAGE_USAGE_COUNT_INVALID",
        "协议包引用计数超过应用可表示范围。",
    )
}

#[cfg(test)]
#[path = "protocol_package_usage/tests.rs"]
mod tests;
