//! Read-only application backup prepare and bounded preview use case.

use super::Application;
use crate::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppError, AppResult, ApplicationBackupImportBaseline,
    ApplicationBackupImportCommitOutcome, ApplicationBackupImportPreparePort,
    ApplicationBackupImportPreview, ApplicationBackupImportToken, ApplicationBackupPackagePreview,
    ApplicationBackupReplacementScope, ApplicationBackupWorkspaceBaseline,
    ApplicationConfigurationDocument, PreparedApplicationBackup,
    validate_portable_protocol_bindings,
};

impl Application {
    pub async fn application_backup_import_commit(
        &self,
        source: &dyn ApplicationBackupImportPreparePort,
        token: ApplicationBackupImportToken,
    ) -> AppResult<ApplicationBackupImportCommitOutcome> {
        let prepared = source.take(token).await?;
        let _gate = self.mutation_gate.lock().await;
        let current_baseline = self.application_backup_import_baseline_locked().await?;
        if current_baseline != prepared.baseline {
            return Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_STALE",
                "预览后应用数据已变化，请重新选择备份并预览。",
            ));
        }
        self.workspaces_before_replacement().await?;

        let candidate = prepared.candidate;
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id: candidate.selected_workspace_id,
            workspaces: candidate.workspaces,
            settings: candidate.settings,
            certificate_materials: candidate.certificate_materials,
            protocol_packages: candidate.protocol_packages,
        };
        document.validate()?;
        self.validate_application_backup_candidate(&document)
            .await?;
        let outcome = ApplicationBackupImportCommitOutcome {
            workspace_count: document.workspaces.len(),
            protocol_package_count: document.protocol_packages.len(),
            enabled_protocol_package_count: document
                .protocol_packages
                .iter()
                .filter(|package| package.enabled)
                .count(),
            portable_material_count: document.certificate_materials.len(),
            requires_restart: true,
        };
        self.restore_and_replace_configuration(document).await?;
        *self.android_package_cache.lock().await = None;
        Ok(outcome)
    }

    /// Performs bulk restore and preflight outside the mutation gate, then briefly
    /// holds it only to freeze an authoritative baseline. Prepare performs no writes.
    pub async fn application_backup_import_prepare(
        &self,
        source: &dyn ApplicationBackupImportPreparePort,
        bytes: Vec<u8>,
    ) -> AppResult<ApplicationBackupImportPreview> {
        let candidate = source.read(bytes).await?;
        let crate::ApplicationBackupImportCandidate {
            selected_workspace_id,
            workspaces,
            settings,
            protocol_packages,
            certificate_materials,
        } = candidate;
        let document = ApplicationConfigurationDocument {
            format_version: APPLICATION_CONFIGURATION_FORMAT_VERSION,
            selected_workspace_id,
            workspaces,
            settings,
            certificate_materials,
            protocol_packages,
        };
        document.validate()?;
        self.validate_application_backup_candidate(&document)
            .await?;

        let protocol_packages = document
            .protocol_packages
            .iter()
            .map(|package| ApplicationBackupPackagePreview {
                package: package.package.clone(),
                enabled: package.enabled,
            })
            .collect::<Vec<_>>();
        let preview_data = (
            document.workspaces.len(),
            document.protocol_packages.len(),
            document
                .protocol_packages
                .iter()
                .filter(|package| package.enabled)
                .count(),
            document.certificate_materials.len(),
        );
        let baseline = self.application_backup_import_baseline().await?;
        let candidate = crate::ApplicationBackupImportCandidate {
            selected_workspace_id: document.selected_workspace_id,
            workspaces: document.workspaces,
            settings: document.settings,
            protocol_packages: document.protocol_packages,
            certificate_materials: document.certificate_materials,
        };
        let (token, expires_in) = source
            .retain(PreparedApplicationBackup {
                candidate,
                baseline,
            })
            .await?;
        Ok(ApplicationBackupImportPreview {
            token,
            expires_in_seconds: expires_in.as_secs(),
            workspace_count: preview_data.0,
            protocol_package_count: preview_data.1,
            enabled_protocol_package_count: preview_data.2,
            portable_material_count: preview_data.3,
            protocol_packages,
            replacement_scope: ApplicationBackupReplacementScope {
                replaces_all_workspaces: true,
                replaces_selected_workspace: true,
                replaces_portable_settings: true,
                replaces_protocol_package_registry: true,
            },
        })
    }

    async fn validate_application_backup_candidate(
        &self,
        document: &ApplicationConfigurationDocument,
    ) -> AppResult<()> {
        let expected_packages = document
            .protocol_packages
            .iter()
            .map(|package| package.package.clone())
            .collect::<Vec<_>>();
        let descriptions = self
            .protocol_package_portability
            .preflight_application_packages(&document.protocol_packages)
            .await?;
        validate_portable_protocol_bindings(
            &document.workspaces,
            &expected_packages,
            &descriptions,
        )?;

        let settings_validation = self
            .settings
            .validate(&document.settings.to_draft(None))
            .await?;
        if !settings_validation.valid {
            return Err(AppError::field(
                "APPLICATION_BACKUP_IMPORT_INVALID",
                "应用备份中的全局 Settings 未通过校验。",
                settings_validation.field_errors,
            ));
        }
        for material in &document.certificate_materials {
            self.listener_certificates
                .preflight_portable(material)
                .await?;
        }
        Ok(())
    }

    pub async fn application_backup_import_discard(
        &self,
        source: &dyn ApplicationBackupImportPreparePort,
        token: ApplicationBackupImportToken,
    ) -> AppResult<()> {
        source.discard(token).await
    }

    pub(super) async fn application_backup_import_baseline(
        &self,
    ) -> AppResult<ApplicationBackupImportBaseline> {
        let _gate = self.mutation_gate.lock().await;
        self.application_backup_import_baseline_locked().await
    }

    pub(super) async fn application_backup_import_baseline_locked(
        &self,
    ) -> AppResult<ApplicationBackupImportBaseline> {
        let summaries = self.workspaces.list().await?;
        let selected_workspace_id = summaries
            .iter()
            .find(|workspace| workspace.selected)
            .map(|workspace| workspace.id)
            .ok_or_else(|| AppError::new("WORKSPACE_NOT_SELECTED", "请先选择一个 Workspace。"))?;
        let workspaces = summaries
            .into_iter()
            .map(|workspace| ApplicationBackupWorkspaceBaseline {
                workspace_id: workspace.id,
                revision: intercept_proxy_domain::Revision::new(workspace.revision),
            })
            .collect();
        let settings_revision =
            intercept_proxy_domain::Revision::new(self.settings.get().await?.revision);
        let protocol_packages = self
            .protocol_package_portability
            .application_backup_baseline()
            .await?;
        let listener_certificate_generation = self
            .listener_certificates
            .application_backup_baseline()
            .await?;
        Ok(ApplicationBackupImportBaseline {
            selected_workspace_id,
            workspaces,
            settings_revision,
            protocol_packages,
            listener_certificate_generation,
        })
    }
}
