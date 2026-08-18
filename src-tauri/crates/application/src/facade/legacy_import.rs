//! Compatibility import for legacy JSON files. New backups use the ZIP workflow.

use chrono::Utc;

use super::Application;
use crate::{
    AppError, AppResult, LegacyImportCandidate, LegacyImportKind, LegacyImportPreparePort,
    LegacyImportPreview, LegacyImportToken, OperationResultViewModel, UiEventPayload, UiTone,
    WORKSPACE_DOCUMENT_V4_FORMAT_VERSION, WorkspaceChangeKind,
    parse_application_configuration_with_source, parse_workspace_document_with_source,
    remap_workspace_identity,
};

impl Application {
    pub async fn legacy_application_configuration_import_prepare(
        &self,
        pending: &dyn LegacyImportPreparePort,
        bytes: Vec<u8>,
    ) -> AppResult<LegacyImportPreview> {
        reject_zip(&bytes)?;
        let parsed = parse_application_configuration_with_source(&bytes)?;
        require_legacy_version(parsed.source_version)?;
        self.prepare_configuration_import(parsed.source_version, parsed.document.clone())
            .await?;
        let baseline = self.application_backup_import_baseline().await?;
        let preview = legacy_preview(
            pending,
            LegacyImportCandidate::ApplicationConfiguration {
                source_version: parsed.source_version,
                document: parsed.document,
                migration_report: parsed.migration_report.clone(),
                baseline,
            },
            LegacyImportKind::ApplicationConfiguration,
            parsed.source_version,
            parsed.migration_report,
        )
        .await?;
        Ok(preview)
    }

    pub async fn legacy_workspace_import_prepare(
        &self,
        pending: &dyn LegacyImportPreparePort,
        bytes: Vec<u8>,
    ) -> AppResult<LegacyImportPreview> {
        reject_zip(&bytes)?;
        let parsed = parse_workspace_document_with_source(&bytes)?;
        require_legacy_version(parsed.source_version)?;
        self.preflight_legacy_workspace(parsed.source_version, &parsed.document)
            .await?;
        legacy_preview(
            pending,
            LegacyImportCandidate::Workspace {
                source_version: parsed.source_version,
                document: parsed.document,
                migration_report: parsed.migration_report.clone(),
            },
            LegacyImportKind::Workspace,
            parsed.source_version,
            parsed.migration_report,
        )
        .await
    }

    pub async fn legacy_application_configuration_import_commit(
        &self,
        pending: &dyn LegacyImportPreparePort,
        token: LegacyImportToken,
    ) -> AppResult<OperationResultViewModel> {
        let candidate = pending.take(token).await?;
        let LegacyImportCandidate::ApplicationConfiguration {
            source_version,
            document,
            migration_report,
            baseline,
        } = candidate
        else {
            return Err(kind_mismatch());
        };
        let _gate = self.mutation_gate.lock().await;
        if self.application_backup_import_baseline_locked().await? != baseline {
            return Err(AppError::new(
                "APPLICATION_BACKUP_IMPORT_STALE",
                "预览后应用数据已变化，请重新选择旧版配置并预览。",
            ));
        }
        let old_workspaces = self.workspaces_before_replacement().await?;
        let (_, document) = self
            .prepare_configuration_import(source_version, document)
            .await?;
        let imported = self
            .restore_and_replace_configuration(source_version, document)
            .await?;
        let cleanup_warning = self
            .discard_replaced_certificate_materials(&old_workspaces, &imported)
            .await
            .err();
        if let Some(error) = &cleanup_warning {
            self.events.publish(
                None,
                Utc::now(),
                None,
                None,
                UiEventPayload::ResourceWarning {
                    message: error.view_model.message.clone(),
                },
            );
        }
        let (message, ui_tone) = super::configuration::import_result_message(
            cleanup_warning.as_ref(),
            migration_report.warning_message().as_deref(),
        );
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message,
            ui_tone,
            entity_id: None,
            revision: None,
            requires_restart: true,
        })
    }

    pub async fn legacy_workspace_import_commit(
        &self,
        pending: &dyn LegacyImportPreparePort,
        token: LegacyImportToken,
    ) -> AppResult<OperationResultViewModel> {
        let candidate = pending.take(token).await?;
        let LegacyImportCandidate::Workspace {
            source_version,
            mut document,
            migration_report,
        } = candidate
        else {
            return Err(kind_mismatch());
        };
        let _gate = self.mutation_gate.lock().await;
        self.preflight_legacy_workspace(source_version, &document)
            .await?;
        let restored = self
            .restore_certificate_materials(
                std::slice::from_mut(&mut document.workspace),
                document.certificate_materials,
            )
            .await?;
        if let Err(error) = remap_workspace_identity(&mut document.workspace) {
            return Err(match self.rollback_restored_certificates(&restored).await {
                Ok(()) => error,
                Err(cleanup) => {
                    super::certificate_portability::certificate_operation_cleanup_error(
                        error, cleanup,
                    )
                }
            });
        }
        let imported = document.workspace.clone();
        let commit = if source_version >= WORKSPACE_DOCUMENT_V4_FORMAT_VERSION {
            self.protocol_package_portability
                .commit_workspace_bundle(document.protocol_packages, document.workspace)
                .await
        } else {
            self.protocol_package_portability
                .commit_legacy_workspace(document.workspace)
                .await
        };
        if let Err(error) = commit {
            return Err(match self.rollback_restored_certificates(&restored).await {
                Ok(()) => error,
                Err(cleanup) => {
                    super::certificate_portability::certificate_operation_cleanup_error(
                        error, cleanup,
                    )
                }
            });
        }
        self.publish_workspace(&imported, false, WorkspaceChangeKind::Imported);
        Ok(OperationResultViewModel {
            success: true,
            cancelled: false,
            message: migration_report.warning_message().unwrap_or_else(|| {
                "旧版 Workspace 与证书材料已导入，并已生成新的本地标识。".into()
            }),
            ui_tone: if migration_report.removed_metadata_extractors > 0 {
                UiTone::Warning
            } else {
                UiTone::Positive
            },
            entity_id: Some(imported.id.to_string()),
            revision: Some(imported.revision.get()),
            requires_restart: false,
        })
    }

    pub async fn legacy_import_discard(
        &self,
        pending: &dyn LegacyImportPreparePort,
        token: LegacyImportToken,
    ) -> AppResult<()> {
        pending.discard(token).await
    }

    async fn preflight_legacy_workspace(
        &self,
        source_version: u16,
        document: &crate::WorkspaceDocument,
    ) -> AppResult<()> {
        let has_package_payload = source_version >= WORKSPACE_DOCUMENT_V4_FORMAT_VERSION;
        let expected = if has_package_payload {
            document
                .protocol_packages
                .iter()
                .map(|package| package.package.clone())
                .collect::<Vec<_>>()
        } else {
            super::protocol_package_portability::referenced_protocol_packages(std::slice::from_ref(
                &document.workspace,
            ))
        };
        let descriptions = if has_package_payload {
            self.protocol_package_portability
                .preflight_workspace_packages(&document.protocol_packages)
                .await?
        } else {
            self.describe_installed_portable_references(std::slice::from_ref(&document.workspace))
                .await?
        };
        crate::validate_portable_protocol_bindings(
            std::slice::from_ref(&document.workspace),
            &expected,
            &descriptions,
        )
    }
}

async fn legacy_preview(
    pending: &dyn LegacyImportPreparePort,
    candidate: LegacyImportCandidate,
    kind: LegacyImportKind,
    source_version: u16,
    migration_report: crate::MigrationReport,
) -> AppResult<LegacyImportPreview> {
    let (workspace_count, material_count) = match &candidate {
        LegacyImportCandidate::ApplicationConfiguration { document, .. } => (
            document.workspaces.len(),
            document.certificate_materials.len(),
        ),
        LegacyImportCandidate::Workspace { document, .. } => {
            (1, document.certificate_materials.len())
        }
    };
    let warnings = migration_report.warning_message().into_iter().collect();
    let (token, ttl) = pending.retain(candidate).await?;
    Ok(LegacyImportPreview {
        token,
        expires_in_seconds: ttl.as_secs(),
        kind,
        source_version,
        workspace_count,
        portable_material_count: material_count,
        migration_report,
        warnings,
    })
}

fn require_legacy_version(version: u16) -> AppResult<()> {
    if (2..=4).contains(&version) {
        Ok(())
    } else {
        Err(AppError::new(
            "LEGACY_IMPORT_VERSION_UNSUPPORTED",
            "兼容导入仅接受版本 2、3、4 的旧版 JSON 文件。",
        ))
    }
}

fn reject_zip(bytes: &[u8]) -> AppResult<()> {
    if bytes.starts_with(b"PK") {
        Err(AppError::new(
            "LEGACY_IMPORT_FORMAT_MISMATCH",
            "兼容导入只接受旧版 JSON 文件，ZIP 请使用应用备份导入。",
        ))
    } else {
        Ok(())
    }
}

fn kind_mismatch() -> AppError {
    AppError::new(
        "LEGACY_IMPORT_KIND_MISMATCH",
        "旧版导入确认令牌与文件类型不匹配。",
    )
}

#[cfg(test)]
mod tests {
    use super::{reject_zip, require_legacy_version};

    #[test]
    fn compatibility_entry_accepts_only_legacy_versions() {
        for version in 2..=4 {
            require_legacy_version(version).expect("supported legacy version");
        }
        assert_eq!(
            require_legacy_version(5).unwrap_err().view_model.code,
            "LEGACY_IMPORT_VERSION_UNSUPPORTED"
        );
    }

    #[test]
    fn compatibility_entry_rejects_zip_without_format_guessing() {
        assert_eq!(
            reject_zip(b"PK\x03\x04archive")
                .unwrap_err()
                .view_model
                .code,
            "LEGACY_IMPORT_FORMAT_MISMATCH"
        );
        reject_zip(br#"{"format_version":4}"#).expect("legacy json candidate");
    }
}
