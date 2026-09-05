//! Listener 编辑态的纯转换与校验入口。

use intercept_proxy_domain::Revision as DomainRevision;

use super::Application;
use crate::{
    AppError, AppResult, CertificateReference, ListenerDraft, ListenerDraftTransition,
    ListenerDraftValidationViewModel, ListenerId, ProtocolPackageRef, WorkspaceId,
};

impl Application {
    pub fn listener_draft_copy(&self, mut source: ListenerDraft) -> AppResult<ListenerDraft> {
        source.id = ListenerId::new();
        source.name = format!("{} 副本", source.name.trim());
        source.enabled = false;
        Ok(source)
    }

    pub fn listener_draft_transition(
        &self,
        draft: ListenerDraft,
        transition: ListenerDraftTransition,
        recommended_package: Option<ProtocolPackageRef>,
    ) -> AppResult<ListenerDraft> {
        draft.transition(transition, recommended_package)
    }

    pub async fn listener_draft_validate(
        &self,
        workspace_id: WorkspaceId,
        expected_workspace_revision: u64,
        draft: ListenerDraft,
        certificate_references: Vec<CertificateReference>,
    ) -> AppResult<ListenerDraftValidationViewModel> {
        let _gate = self.mutation_gate.lock().await;
        let workspace = self.workspaces.get(workspace_id).await?;
        workspace
            .revision
            .verify(DomainRevision::new(expected_workspace_revision))
            .map_err(AppError::from)?;

        let listener_index = workspace
            .listeners
            .iter()
            .position(|listener| listener.id == draft.id)
            .unwrap_or(workspace.listeners.len());
        let listener = match draft.try_into_listener(listener_index) {
            Ok(listener) => listener,
            Err(field_errors) => {
                return Ok(ListenerDraftValidationViewModel {
                    valid: false,
                    normalized: None,
                    field_errors,
                });
            }
        };
        let validation = self
            .listener_validation_from_workspace(
                workspace,
                expected_workspace_revision,
                listener,
                certificate_references,
            )
            .await?;
        if validation.valid {
            Ok(ListenerDraftValidationViewModel {
                valid: true,
                normalized: Some(validation.normalized),
                field_errors: validation.field_errors,
            })
        } else {
            Ok(ListenerDraftValidationViewModel {
                valid: false,
                normalized: None,
                field_errors: validation.field_errors,
            })
        }
    }
}
