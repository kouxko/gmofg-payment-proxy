//! Workspace 与 Listener 运行态到协议包引用查询端口的适配器。
//!
//! 查询始终遍历全部 Workspace，并且只用完整 [`ProtocolPackageRef`] 相等判断。协议包 ID
//! 相同但版本不同、版本文本相同但 ID 不同，都不会被误判为当前版本的使用者。

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use intercept_proxy_application::{
    AppResult, ListenerRuntimePort, ListenerRuntimeState, ProtocolPackageRef,
    ProtocolPackageUsageQueryPort, ProtocolPackageUsageViewModel, WorkspaceRepositoryPort,
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
}

#[async_trait]
impl ProtocolPackageUsageQueryPort for ProtocolPackageUsageQueryAdapter {
    async fn usages(
        &self,
        package: &ProtocolPackageRef,
    ) -> AppResult<Vec<ProtocolPackageUsageViewModel>> {
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
                if &scripted.package != package {
                    continue;
                }
                usages.push(ProtocolPackageUsageViewModel {
                    workspace_id: workspace.id,
                    workspace_name: workspace.name.clone(),
                    listener_id: listener.id,
                    listener_name: listener.name.clone(),
                    listener_enabled: listener.enabled,
                    runtime_state: runtime_states
                        .get(&listener.id)
                        .cloned()
                        .unwrap_or(ListenerRuntimeState::Stopped),
                });
            }
        }
        usages.sort_by(|left, right| {
            left.workspace_id
                .cmp(&right.workspace_id)
                .then_with(|| left.listener_id.cmp(&right.listener_id))
        });
        Ok(usages)
    }
}

#[cfg(test)]
#[path = "protocol_package_usage/tests.rs"]
mod tests;
