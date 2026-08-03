//! Workspace 应用用例。
//!
//! 文件选择、仓储和事件发布全部在 Rust 门面编排。展示层只调用无路径、无字节的命令，
//! 因而同一组用例可以由 Tauri、未来 CLI/TUI 或无界面测试复用。

use chrono::Utc;
use intercept_proxy_domain::{
    CertificateReference, CertificateReferenceId, CertificateReferenceKind, ConnectionFaultAction,
    FaultPreset, FaultPresetId, ListenerId, MetadataExtractor, MetadataExtractorId,
    MetadataExtractorSource, ResponseAssertion, ResponseAssertionId, ResponseAssertionKind,
};
use uuid::Uuid;

use super::Application;
use crate::{
    AppError, AppResult, OperationResultViewModel, ProxyWorkspace, UiEventPayload, UiTone,
    WorkspaceChangeKind, WorkspaceChangedViewModel, WorkspaceId, WorkspaceSummaryViewModel,
    WorkspaceValidationViewModel,
};

impl Application {
    /// 在未保存的 Workspace 草稿中追加一个由 Rust 生成稳定 ID 的通用组件。
    ///
    /// 前端不得自行生成领域 ID；同时该命令不持久化草稿，调用者仍需执行
    /// `workspace_validate` 与 `workspace_save`。证书引用故意以空引用开始，确保用户
    /// 明确选择安全材料后才能通过最终校验。
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
                workspace.certificate_references.push(CertificateReference {
                    id: CertificateReferenceId::new(),
                    label: "Certificate Reference".into(),
                    kind: CertificateReferenceKind::ReverseServerIdentity,
                    reference: String::new(),
                });
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
        self.ensure_workspace_not_running(&current).await?;
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

    /// 打开系统文件选择器并导入 Workspace；路径和文档字节不会进入前端。
    pub async fn workspace_import(&self) -> AppResult<OperationResultViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let Some(document) = self.workspace_documents.pick_import_document().await? else {
            return Ok(cancelled("已取消导入 Workspace。"));
        };
        let workspace = self.workspaces.import_document(document).await?;
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
            message: "Workspace 已导入。".into(),
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
        let workspace = self.workspaces.get(workspace_id).await?;
        let document = self.workspaces.export_document(workspace_id).await?;
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
            message: "Workspace 已导出。".into(),
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

fn find_mut<'a, T>(
    items: &'a mut [T],
    component_id: &str,
    id: impl Fn(&T) -> String,
) -> AppResult<&'a mut T> {
    items
        .iter_mut()
        .find(|item| id(item) == component_id)
        .ok_or_else(|| {
            AppError::new(
                "WORKSPACE_COMPONENT_NOT_FOUND",
                "Workspace 组件不存在或已被删除。",
            )
            .entity(component_id.to_owned())
        })
}

fn parse_listener_ids(raw: &str) -> AppResult<Vec<ListenerId>> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            Uuid::parse_str(value)
                .map(ListenerId::from_uuid)
                .map_err(|_| {
                    AppError::new(
                        "WORKSPACE_LISTENER_ID_INVALID",
                        format!("代理入口 ID“{value}”不是有效 UUID。"),
                    )
                })
        })
        .collect()
}

fn metadata_source(kind: &str) -> AppResult<MetadataExtractorSource> {
    match kind {
        "header" => Ok(MetadataExtractorSource::Header {
            name: String::new(),
        }),
        "json_path" => Ok(MetadataExtractorSource::JsonPath {
            path: "$.field".into(),
        }),
        "body_text" => Ok(MetadataExtractorSource::BodyText),
        "fixed_value" => Ok(MetadataExtractorSource::FixedValue {
            value: String::new(),
        }),
        _ => Err(component_variant_error()),
    }
}

fn response_assertion(kind: &str) -> AppResult<ResponseAssertionKind> {
    match kind {
        "http_status_equals" => Ok(ResponseAssertionKind::HttpStatusEquals { expected: 200 }),
        "header_equals" => Ok(ResponseAssertionKind::HeaderEquals {
            name: String::new(),
            expected: String::new(),
        }),
        "json_path_equals" => Ok(ResponseAssertionKind::JsonPathEquals {
            path: "$.field".into(),
            expected: serde_json::Value::Null,
        }),
        "body_text_contains" => Ok(ResponseAssertionKind::BodyTextContains {
            expected: String::new(),
        }),
        "body_length_equals" => Ok(ResponseAssertionKind::BodyLengthEquals { expected: 0 }),
        "body_sha256_equals" => Ok(ResponseAssertionKind::BodySha256Equals {
            expected_hex: String::new(),
        }),
        _ => Err(component_variant_error()),
    }
}

fn connection_fault(kind: &str) -> AppResult<ConnectionFaultAction> {
    match kind {
        "delay" => Ok(ConnectionFaultAction::Delay { milliseconds: 100 }),
        "reject" => Ok(ConnectionFaultAction::Reject),
        "rate_limit" => Ok(ConnectionFaultAction::RateLimit {
            bytes_per_second: 64 * 1024,
        }),
        "close_after_bytes" => Ok(ConnectionFaultAction::CloseAfterBytes { bytes: 1 }),
        "half_close_after_bytes" => Ok(ConnectionFaultAction::HalfCloseAfterBytes { bytes: 1 }),
        "idle_timeout" => Ok(ConnectionFaultAction::IdleTimeout {
            milliseconds: 30_000,
        }),
        _ => Err(component_variant_error()),
    }
}

fn component_variant_error() -> AppError {
    AppError::new(
        "WORKSPACE_COMPONENT_VARIANT_INVALID",
        "Workspace 组件类型选项无效。",
    )
}

fn delete_component(
    workspace: &mut ProxyWorkspace,
    component_kind: &str,
    component_id: &str,
) -> AppResult<()> {
    let removed = match component_kind {
        "metadata_extractor" => {
            retain_removed(&mut workspace.metadata_extractors, component_id, |item| {
                item.id.to_string()
            })
        }
        "response_assertion" => {
            retain_removed(&mut workspace.response_assertions, component_id, |item| {
                item.id.to_string()
            })
        }
        "fault_preset" => retain_removed(&mut workspace.fault_presets, component_id, |item| {
            item.id.to_string()
        }),
        "certificate_reference" => retain_removed(
            &mut workspace.certificate_references,
            component_id,
            |item| item.id.to_string(),
        ),
        _ => return Err(component_variant_error()),
    };
    if removed {
        Ok(())
    } else {
        Err(AppError::new(
            "WORKSPACE_COMPONENT_NOT_FOUND",
            "Workspace 组件不存在或已被删除。",
        )
        .entity(component_id.to_owned()))
    }
}

fn retain_removed<T>(items: &mut Vec<T>, component_id: &str, id: impl Fn(&T) -> String) -> bool {
    let before = items.len();
    items.retain(|item| id(item) != component_id);
    items.len() != before
}

fn cancelled(message: &str) -> OperationResultViewModel {
    OperationResultViewModel {
        success: false,
        cancelled: true,
        message: message.into(),
        ui_tone: UiTone::Neutral,
        entity_id: None,
        revision: None,
        requires_restart: false,
    }
}

fn safe_file_stem(name: &str) -> String {
    let value = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "workspace".into()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::safe_file_stem;

    #[test]
    fn export_file_name_cannot_escape_selected_directory() {
        assert_eq!(safe_file_stem("../Lab Workspace"), ".._Lab_Workspace");
        assert_eq!(safe_file_stem("  "), "workspace");
    }
}
