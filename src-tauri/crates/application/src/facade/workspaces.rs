//! Workspace 应用用例。
//!
//! 文件选择、仓储和事件发布全部在 Rust 门面编排。展示层只调用无路径、无字节的命令，
//! 因而同一组用例可以由 Tauri、未来 CLI/TUI 或无界面测试复用。

use chrono::Utc;
use intercept_proxy_domain::{
    ConnectionFaultAction, FaultPreset, FaultPresetId, ListenerId, MetadataExtractor,
    MetadataExtractorId, MetadataExtractorSource, ResponseAssertion, ResponseAssertionId,
    ResponseAssertionKind,
};
use uuid::Uuid;

use super::Application;
use crate::{
    AndroidNetworkState, AppError, AppResult, OperationResultViewModel, ProxyWorkspace,
    UiEventPayload, UiTone, WORKSPACE_DOCUMENT_FORMAT_VERSION, WorkspaceChangeKind,
    WorkspaceChangedViewModel, WorkspaceDocument, WorkspaceId, WorkspaceSummaryViewModel,
    WorkspaceValidationViewModel, parse_workspace_document,
    retain_reachable_certificate_references, serialize_workspace_document,
};

impl Application {
    /// 在未保存的 Workspace 草稿中追加一个由 Rust 生成稳定 ID 的通用组件。
    ///
    /// 前端不得自行生成领域 ID；同时该命令不持久化草稿，调用者仍需执行
    /// `workspace_validate` 与 `workspace_save`。证书引用只能通过 Listener 证书导入端口
    /// 创建，不能用任意路径文本构造。
    pub fn workspace_component_new(
        &self,
        mut workspace: ProxyWorkspace,
        kind: &str,
    ) -> AppResult<ProxyWorkspace> {
        match kind {
            "metadata_extractor" => workspace.metadata_extractors.push(MetadataExtractor {
                id: MetadataExtractorId::new(),
                name: "Metadata Extractor".into(),
                listener_ids: Vec::new(),
                source: MetadataExtractorSource::BodyText,
            }),
            "response_assertion" => workspace.response_assertions.push(ResponseAssertion {
                id: ResponseAssertionId::new(),
                name: "Response Assertion".into(),
                listener_ids: Vec::new(),
                enabled: true,
                assertion: ResponseAssertionKind::HttpStatusEquals { expected: 200 },
            }),
            "fault_preset" => workspace.fault_presets.push(FaultPreset {
                id: FaultPresetId::new(),
                name: "Connection Fault Preset".into(),
                description: String::new(),
                connection_actions: vec![ConnectionFaultAction::Delay { milliseconds: 100 }],
                http_actions: Vec::new(),
            }),
            "certificate_reference" => {
                return Err(AppError::new(
                    "WORKSPACE_CERTIFICATE_IMPORT_REQUIRED",
                    "证书材料必须在代理入口中按用途导入。",
                ));
            }
            _ => {
                return Err(AppError::new(
                    "WORKSPACE_COMPONENT_KIND_INVALID",
                    "Workspace 组件类型无效。",
                ));
            }
        }
        Ok(workspace)
    }

    /// 对 Workspace 组件执行会改变领域结构的编辑意图。
    ///
    /// 前端只提交组件、意图和原始文本；联合类型默认值、Listener ID 解析与删除行为都
    /// 在 Rust 中完成，因此桌面 UI、未来 CLI/TUI 和无界面测试共享同一套语义。
    pub fn workspace_component_apply_intent(
        &self,
        mut workspace: ProxyWorkspace,
        component_kind: &str,
        component_id: &str,
        operation: &str,
        value: &str,
    ) -> AppResult<ProxyWorkspace> {
        if operation == "delete" {
            delete_component(&mut workspace, component_kind, component_id)?;
            return Ok(workspace);
        }
        match (component_kind, operation) {
            ("metadata_extractor", "listener_ids") => {
                let ids = parse_listener_ids(value)?;
                find_mut(&mut workspace.metadata_extractors, component_id, |item| {
                    item.id.to_string()
                })?
                .listener_ids = ids;
            }
            ("response_assertion", "listener_ids") => {
                let ids = parse_listener_ids(value)?;
                find_mut(&mut workspace.response_assertions, component_id, |item| {
                    item.id.to_string()
                })?
                .listener_ids = ids;
            }
            ("metadata_extractor", "variant") => {
                find_mut(&mut workspace.metadata_extractors, component_id, |item| {
                    item.id.to_string()
                })?
                .source = metadata_source(value)?;
            }
            ("response_assertion", "variant") => {
                find_mut(&mut workspace.response_assertions, component_id, |item| {
                    item.id.to_string()
                })?
                .assertion = response_assertion(value)?;
            }
            ("fault_preset", "variant") => {
                find_mut(&mut workspace.fault_presets, component_id, |item| {
                    item.id.to_string()
                })?
                .connection_actions = vec![connection_fault(value)?];
            }
            _ => {
                return Err(AppError::new(
                    "WORKSPACE_COMPONENT_INTENT_INVALID",
                    "Workspace 组件编辑意图无效。",
                ));
            }
        }
        Ok(workspace)
    }

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

        let status = self.android.network_status().await.map_err(|error| {
            AppError::new(
                "WORKSPACE_ANDROID_STATUS_UNAVAILABLE",
                format!(
                    "无法确认 Workspace 的设备网络方案是否仍在运行：{}",
                    error.view_model.message
                ),
            )
            .retryable("请连接目标设备并刷新 VPN 状态，或先执行紧急恢复网络。")
        })?;
        let active = matches!(
            status.state,
            AndroidNetworkState::StartRequested
                | AndroidNetworkState::Running
                | AndroidNetworkState::StopRequested
        ) && status.active_profile_id.as_ref().is_some_and(|active_id| {
            workspace
                .android_network_profiles
                .iter()
                .any(|profile| profile.id == *active_id)
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
        observation_required: bool,
    ) -> AppResult<()> {
        let status = match self.android.network_status().await {
            Ok(status) => status,
            Err(error)
                if !observation_required
                    && matches!(
                        error.view_model.code.as_str(),
                        "ANDROID_CONTROL_UNAVAILABLE" | "ANDROID_DEVICE_NOT_SELECTED"
                    ) =>
            {
                return Ok(());
            }
            Err(error) => {
                return Err(AppError::new(
                    "WORKSPACE_ANDROID_STATUS_UNAVAILABLE",
                    format!(
                        "无法确认设备网络接管是否已经停止：{}",
                        error.view_model.message
                    ),
                )
                .retryable("请连接目标设备并刷新 VPN 状态，或先执行紧急恢复网络。"));
            }
        };
        if matches!(
            status.state,
            AndroidNetworkState::StartRequested
                | AndroidNetworkState::Running
                | AndroidNetworkState::StopRequested
        ) {
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
                "Workspace 存在运行中的 Listener；请停止后再保存或删除配置。",
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

    /// 打开系统文件选择器并导入 Workspace；路径和文档字节不会进入前端。
    pub async fn workspace_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let Some(document) = self.workspace_documents.pick_import_document().await? else {
            return Ok(cancelled("已取消导入 Workspace。"));
        };
        let parsed = parse_workspace_document(&document)?;
        let mut workspaces = vec![parsed.workspace];
        let restored = self
            .restore_certificate_materials(&mut workspaces, parsed.certificate_materials)
            .await?;
        let workspace = match self.workspaces.import_workspace(workspaces.remove(0)).await {
            Ok(workspace) => workspace,
            Err(error) => {
                return Err(match self.rollback_restored_certificates(&restored).await {
                    Ok(()) => error,
                    Err(cleanup) => {
                        super::certificate_portability::certificate_operation_cleanup_error(
                            error, cleanup,
                        )
                    }
                });
            }
        };
        let selected = self
            .workspaces
            .list()
            .await?
            .iter()
            .any(|summary| summary.id == workspace.id && summary.selected);
        self.publish_workspace(&workspace, selected, WorkspaceChangeKind::Imported);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Workspace 与证书材料已从单个文件导入。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(workspace.id.to_string()),
            revision: Some(workspace.revision.get()),
            requires_restart: false,
        })
    }

    /// 生成安全文档并打开系统保存对话框；前端不会收到路径或文档字节。
    pub async fn workspace_export(
        &self,
        workspace_id: WorkspaceId,
    ) -> AppResult<OperationResultViewModel> {
        let mut workspace = self.workspaces.get(workspace_id).await?;
        retain_reachable_certificate_references(&mut workspace);
        let certificate_materials = self
            .export_certificate_materials(std::slice::from_ref(&workspace))
            .await?;
        let document = serialize_workspace_document(&WorkspaceDocument {
            format_version: WORKSPACE_DOCUMENT_FORMAT_VERSION,
            workspace: workspace.clone(),
            certificate_materials,
        })?;
        let suggested_file_name =
            format!("{}.intercept-workspace", safe_file_stem(&workspace.name));
        if !self
            .workspace_documents
            .save_export_document(suggested_file_name, document)
            .await?
        {
            return Ok(cancelled("已取消导出 Workspace。"));
        }
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: "Workspace 与证书材料已导出到单个文件。".into(),
            ui_tone: UiTone::Positive,
            entity_id: Some(workspace.id.to_string()),
            revision: Some(workspace.revision.get()),
            requires_restart: false,
        })
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

mod support;
use support::{
    cancelled, connection_fault, delete_component, find_mut, metadata_source, parse_listener_ids,
    response_assertion, safe_file_stem,
};

#[cfg(test)]
#[path = "workspaces_tests.rs"]
mod tests;
