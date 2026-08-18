//! Read-only application backup prepare and bounded preview use case.

use super::Application;
use crate::{
    APPLICATION_CONFIGURATION_FORMAT_VERSION, AppError, AppResult, ApplicationBackupImportBaseline,
    ApplicationBackupImportPreparePort, ApplicationBackupImportPreview,
    ApplicationBackupImportToken, ApplicationBackupPackagePreview,
    ApplicationBackupReplacementScope, ApplicationBackupWorkspaceBaseline,
    ApplicationConfigurationDocument, PreparedApplicationBackup,
    validate_portable_protocol_bindings,
};

impl Application {
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
            migration_report,
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
        let warnings = migration_report.warning_message().into_iter().collect();
        let preview_data = (
            document.workspaces.len(),
            document.protocol_packages.len(),
            document
                .protocol_packages
                .iter()
                .filter(|package| package.enabled)
                .count(),
            document.certificate_materials.len(),
            migration_report.clone(),
        );
        let baseline = self.application_backup_import_baseline().await?;
        let candidate = crate::ApplicationBackupImportCandidate {
            selected_workspace_id: document.selected_workspace_id,
            workspaces: document.workspaces,
            settings: document.settings,
            protocol_packages: document.protocol_packages,
            certificate_materials: document.certificate_materials,
            migration_report,
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
            migration_report: preview_data.4,
            warnings,
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

    async fn application_backup_import_baseline(
        &self,
    ) -> AppResult<ApplicationBackupImportBaseline> {
        let _gate = self.mutation_gate.lock().await;
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
