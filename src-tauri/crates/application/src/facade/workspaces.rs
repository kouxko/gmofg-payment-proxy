//! Workspace 应用用例。
//!
//! 文件选择、仓储和事件发布全部在 Rust 门面编排。展示层只调用无路径、无字节的命令，
//! 因而同一组用例可以由 Tauri、未来 CLI/TUI 或无界面测试复用。

use chrono::Utc;

use super::Application;
use crate::{
    AppError, AppResult, OperationResultViewModel, ProxyWorkspace, UiEventPayload,
    WorkspaceChangeKind, WorkspaceChangedViewModel, WorkspaceId, WorkspaceSummaryViewModel,
    WorkspaceValidationViewModel,
};

impl Application {
    pub async fn workspace_list(&self) -> AppResult<Vec<WorkspaceSummaryViewModel>> {
        self.workspaces.list().await
    }

    pub async fn workspace_get(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        self.workspaces.get(workspace_id).await
    }

    pub async fn workspace_create(&self, name: String) -> AppResult<ProxyWorkspace> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.create(name).await?;
        self.publish_workspace(&workspace, false, WorkspaceChangeKind::Created);
        Ok(workspace)
    }

    pub async fn workspace_copy(&self, workspace_id: WorkspaceId) -> AppResult<ProxyWorkspace> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.copy(workspace_id).await?;
        self.publish_workspace(&workspace, false, WorkspaceChangeKind::Created);
        Ok(workspace)
    }

    pub async fn workspace_select(
        &self,
        workspace_id: WorkspaceId,
    ) -> AppResult<WorkspaceSummaryViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let summary = self.workspaces.select(workspace_id).await?;
        self.publish_workspace_summary(summary.clone(), WorkspaceChangeKind::Selected);
        Ok(summary)
    }

    pub async fn workspace_validate(
        &self,
        workspace: ProxyWorkspace,
    ) -> AppResult<WorkspaceValidationViewModel> {
        self.workspaces.validate(workspace).await
    }

    pub async fn workspace_save(&self, workspace: ProxyWorkspace) -> AppResult<ProxyWorkspace> {
        let _gate = self.mutation_gate.lock().await;
        let current = self.workspaces.get(workspace.id).await?;
        if current.certificate_references != workspace.certificate_references {
            return Err(AppError::new(
                "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED",
                "Workspace 证书引用只能通过代理入口的证书导入功能变更。",
            )
            .entity(workspace.id.to_string()));
        }
        self.ensure_workspace_update_allowed(&current, &workspace)
            .await?;
        let workspace = self.workspaces.save(workspace).await?;
        let selected = self
            .workspaces
            .list()
            .await?
            .iter()
            .any(|summary| summary.id == workspace.id && summary.selected);
        self.publish_workspace(&workspace, selected, WorkspaceChangeKind::Updated);
        Ok(workspace)
    }

    pub async fn workspace_delete(
        &self,
        workspace_id: WorkspaceId,
        expected_revision: u64,
    ) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        self.ensure_workspace_not_running(&workspace).await?;
        self.ensure_workspace_android_network_not_running(&workspace)
            .await?;
        let result = self
            .workspaces
            .delete(workspace_id, expected_revision)
            .await?;
        self.events.publish(
            None,
            Utc::now(),
            Some(workspace_id.to_string()),
            Some(expected_revision),
            UiEventPayload::WorkspaceChanged(WorkspaceChangedViewModel {
                workspace_id,
                kind: WorkspaceChangeKind::Deleted,
                summary: None,
            }),
        );
        Ok(result)
    }

    /// 设备网络接管与桌面 Listener 是两个独立运行时。删除 Workspace 前必须分别确认
    /// 两者均未引用该 Workspace，避免设备继续运行一个已经无法编辑或停止的方案。
    pub(crate) async fn ensure_workspace_android_network_not_running(
        &self,
        workspace: &ProxyWorkspace,
    ) -> AppResult<()> {
        if workspace.android_network_profiles.is_empty() {
            return Ok(());
        }

        let owners = self.android.runtime_owners().await.map_err(|error| {
            AppError::new(
                "WORKSPACE_ANDROID_STATUS_UNAVAILABLE",
                format!(
                    "无法确认 Workspace 的设备网络方案是否仍在运行：{}",
                    error.view_model.message
                ),
            )
            .retryable("请连接目标设备并刷新 VPN 状态，或先执行紧急恢复网络。")
        })?;
        let active = owners.iter().any(|owner| {
            workspace
                .android_network_profiles
                .iter()
                .any(|profile| profile.id == owner.profile_id)
        });
        if active {
            return Err(AppError::new(
                "WORKSPACE_ANDROID_NETWORK_ACTIVE",
                "Workspace 的设备网络方案仍在运行，不能删除。",
            )
            .retryable("请先停止设备网络接管，再删除 Workspace。")
            .entity(workspace.id.to_string()));
        }
        Ok(())
    }

    /// 在原子替换全部 Workspace 前确认设备网络运行时已经停止。
    ///
    /// 单个 Workspace 删除可以按 `active_profile_id` 判断归属；完整配置替换则不同：旧
    /// Profile 会全部消失，因此只要 Companion 处于启动、运行或停止过渡态就必须拒绝。
    /// 即使设备回报了已经无法映射到本地配置的陈旧 Profile ID，也不能假定网络接管已
    /// 停止，否则替换后将失去恢复该运行时所需的配置。
    pub(crate) async fn ensure_android_network_replacement_safe(
        &self,
        _observation_required: bool,
    ) -> AppResult<()> {
        let owners = self.android.runtime_owners().await.map_err(|error| {
            AppError::new(
                "WORKSPACE_ANDROID_STATUS_UNAVAILABLE",
                format!(
                    "无法确认设备网络接管是否已经停止：{}",
                    error.view_model.message
                ),
            )
            .retryable("请连接目标设备并刷新 VPN 状态，或先执行紧急恢复网络。")
        })?;
        if !owners.is_empty() {
            return Err(AppError::new(
                "WORKSPACE_ANDROID_NETWORK_ACTIVE",
                "设备网络接管仍在运行或正在切换状态，不能替换完整配置。",
            )
            .retryable("请先停止设备网络接管，再导入完整配置。"));
        }
        Ok(())
    }

    /// Live listeners execute an immutable Workspace snapshot. Reject aggregate mutation while
    /// any listener from that Workspace is active so persisted configuration and live behavior
    /// can never silently diverge.
    pub(crate) async fn ensure_workspace_not_running(
        &self,
        workspace: &ProxyWorkspace,
    ) -> AppResult<()> {
        let running = self.listener_runtime.statuses().await?;
        if let Some(status) = running.iter().find(|status| {
            workspace
                .listeners
                .iter()
                .any(|listener| listener.id == status.listener_id)
        }) {
            return Err(AppError::new(
                "WORKSPACE_RUNTIME_ACTIVE",
                "工作区存在运行中的入口；请停止后再保存或删除配置。",
            )
            .entity(status.listener_id.to_string()));
        }
        Ok(())
    }

    /// 运行中的入口只锁定会参与代理运行快照的 Workspace 配置。
    ///
    /// Android 设备网络方案虽然随 Workspace 导入导出，但由独立的 `VpnService` 运行；
    /// 保存方案不会改变任何已启动 Listener 的监听地址、证书、规则或转发行为。因此仅
    /// 修改 `android_network_profiles` 时允许继续持久化，其余聚合字段仍执行运行态保护。
    pub(crate) async fn ensure_workspace_update_allowed(
        &self,
        current: &ProxyWorkspace,
        proposed: &ProxyWorkspace,
    ) -> AppResult<()> {
        let mut current_runtime_configuration = current.clone();
        let mut proposed_runtime_configuration = proposed.clone();
        current_runtime_configuration
            .android_network_profiles
            .clear();
        proposed_runtime_configuration
            .android_network_profiles
            .clear();
        if current_runtime_configuration == proposed_runtime_configuration {
            return Ok(());
        }
        self.ensure_workspace_not_running(current).await
    }

    pub(crate) fn publish_workspace(
        &self,
        workspace: &ProxyWorkspace,
        selected: bool,
        kind: WorkspaceChangeKind,
    ) {
        self.publish_workspace_summary(
            WorkspaceSummaryViewModel::from_workspace(workspace, selected),
            kind,
        );
    }

    fn publish_workspace_summary(
        &self,
        summary: WorkspaceSummaryViewModel,
        kind: WorkspaceChangeKind,
    ) {
        self.events.publish(
            None,
            Utc::now(),
            Some(summary.id.to_string()),
            Some(summary.revision),
            UiEventPayload::WorkspaceChanged(WorkspaceChangedViewModel {
                workspace_id: summary.id,
                kind,
                summary: Some(summary),
            }),
        );
    }
}
