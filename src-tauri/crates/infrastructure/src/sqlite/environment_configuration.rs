use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::Utc;
use intercept_proxy_application::{
    AppError, AppResult, EnvironmentCommitFailure, EnvironmentCommitPort, EnvironmentCommitRequest,
    EnvironmentCommitResult, EnvironmentCommitRollbackOutcome, EnvironmentCommitTarget,
    EnvironmentConsumedCommitRequest, EnvironmentPreparedMaterialKind,
    EnvironmentPreparedMaterialVisitor, EnvironmentSelectionPolicy, MaterialAlias,
};
use intercept_proxy_domain::{
    ForwardProxyAuthentication, ListenerDataPlane, ProxyWorkspace, Revision, WorkspaceId,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde_json::json;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::environment_configuration_baseline::{
    check_workspace_baseline, exact_certificate_inventory, exact_package_inventory,
    exact_secret_inventory,
};
use super::{SqliteExecutor, SqliteStore, revision_to_i64};
use crate::adapters::{
    PreparedMaterialBatch, PreparedMaterialRecord, common::encode_workspace_record,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentCommitFaultPoint {
    BeforeCertificateInsert,
    BeforeSecretInsert,
    BeforeWorkspaceWrite,
    BeforeSelectionWrite,
    BeforeCommit,
}

struct MaterialWriteSummary {
    references: BTreeMap<String, String>,
    inserted_materials: usize,
    reused_materials: usize,
}

struct WorkspaceWrite {
    workspace_id: Uuid,
    revision: u64,
    preserve_existing_selection: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EnvironmentConfigurationCommitAdapter {
    executor: SqliteExecutor,
}

impl EnvironmentConfigurationCommitAdapter {
    pub(crate) fn new(executor: SqliteExecutor) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl EnvironmentCommitPort for EnvironmentConfigurationCommitAdapter {
    async fn commit(
        &self,
        request: EnvironmentCommitRequest,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure> {
        self.executor
            .execute(move |store| store.commit_environment_configuration(request, None))
            .await
    }
}

impl From<crate::InfrastructureError> for EnvironmentCommitFailure {
    fn from(error: crate::InfrastructureError) -> Self {
        Self::before_transaction(AppError::from(error))
    }
}

impl SqliteStore {
    pub(crate) fn commit_environment_configuration(
        &self,
        request: EnvironmentCommitRequest,
        fault: Option<EnvironmentCommitFaultPoint>,
    ) -> Result<EnvironmentCommitResult, EnvironmentCommitFailure> {
        let mut prepared_materials = PreparedMaterialBatch {
            certificates: Vec::new(),
            secrets: Vec::new(),
        };
        let EnvironmentConsumedCommitRequest {
            baseline,
            target,
            workspace,
            selection_policy,
        } = request
            .consume_with(&mut prepared_materials)
            .map_err(EnvironmentCommitFailure::before_transaction)?;
        let PreparedMaterialBatch {
            certificates: prepared_certificate_handles,
            secrets: prepared_secret_handles,
        } = prepared_materials;
        reject_cross_family_aliases(&prepared_certificate_handles, &prepared_secret_handles)
            .map_err(EnvironmentCommitFailure::before_transaction)?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| EnvironmentCommitFailure::before_transaction(commit_failed()))?;

        let selected_workspace_id =
            selected_workspace_id(&transaction).map_err(rolled_back_failed)?;
        if selected_workspace_id != baseline.selected_workspace_id {
            return Err(EnvironmentCommitFailure::rolled_back(
                baseline_mismatch(),
                EnvironmentCommitRollbackOutcome::BaselineMismatch,
            ));
        }
        check_workspace_baseline(&transaction, &target, &baseline).map_err(rolled_back_failed)?;
        let certificate_inventory =
            check_baseline(&transaction, &baseline).map_err(rolled_back_failed)?;
        let materials = write_materials(
            &transaction,
            prepared_certificate_handles,
            prepared_secret_handles,
            certificate_inventory,
            fault,
        )
        .map_err(rolled_back_failed)?;
        let workspace = rewrite_material_aliases(workspace, &materials.references)
            .map_err(rolled_back_failed)?;
        maybe_fault(fault, EnvironmentCommitFaultPoint::BeforeWorkspaceWrite)
            .map_err(rolled_back_failed)?;
        let workspace =
            write_workspace(&transaction, target, workspace).map_err(rolled_back_failed)?;
        maybe_fault(fault, EnvironmentCommitFaultPoint::BeforeSelectionWrite)
            .map_err(rolled_back_failed)?;
        apply_selection(&transaction, selection_policy, &workspace).map_err(rolled_back_failed)?;
        advance_certificate_inventory(&transaction, &materials.references)
            .map_err(rolled_back_failed)?;
        maybe_fault(fault, EnvironmentCommitFaultPoint::BeforeCommit)
            .map_err(rolled_back_failed)?;
        // transaction.commit() is the only success boundary; no receipt exists before it.
        transaction
            .commit()
            .map_err(|_| rolled_back_failed(commit_rolled_back()))?;

        // Application turns this successful result into EnvironmentCommitReceipt; Infrastructure
        // cannot construct that authority before or after a failed transaction.

        let selected_workspace_id = if workspace.preserve_existing_selection {
            selected_workspace_id
        } else {
            selected_workspace_id.or(Some(workspace.workspace_id))
        };
        Ok(EnvironmentCommitResult {
            workspace_id: workspace.workspace_id,
            revision: workspace.revision,
            selected_workspace_id,
            reused_materials: materials.reused_materials,
            inserted_materials: materials.inserted_materials,
        })
    }
}

impl EnvironmentPreparedMaterialVisitor for PreparedMaterialBatch {
    fn visit(
        &mut self,
        kind: EnvironmentPreparedMaterialKind,
        alias: MaterialAlias,
        fingerprint: [u8; 32],
        protected_payload: Zeroizing<Vec<u8>>,
    ) -> AppResult<()> {
        let record = PreparedMaterialRecord {
            alias,
            fingerprint,
            protected_payload,
        };
        match kind {
            EnvironmentPreparedMaterialKind::Certificate => self.certificates.push(record),
            EnvironmentPreparedMaterialKind::Secret => self.secrets.push(record),
        }
        Ok(())
    }
}

fn check_baseline(
    transaction: &Transaction<'_>,
    baseline: &intercept_proxy_application::EnvironmentApplyGenerations,
) -> AppResult<u64> {
    let package_inventory = exact_package_inventory(transaction)?;
    let certificate_revision = transaction
        .query_row(
            "SELECT revision FROM certificate_state WHERE singleton_id = 1",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|_| commit_failed())?;
    let certificate_revision = u64::try_from(certificate_revision).map_err(|_| commit_failed())?;
    let certificate_inventory = exact_certificate_inventory(transaction, certificate_revision)?;
    let protected_secret_inventory = exact_secret_inventory(transaction)?;
    if certificate_inventory != baseline.certificate_inventory
        || protected_secret_inventory != baseline.protected_secret_inventory
        || package_inventory != baseline.package_inventory
    {
        return Err(baseline_mismatch());
    }
    Ok(certificate_revision)
}

fn write_materials(
    transaction: &Transaction<'_>,
    certificates: Vec<PreparedMaterialRecord>,
    secrets: Vec<PreparedMaterialRecord>,
    certificate_inventory: u64,
    fault: Option<EnvironmentCommitFaultPoint>,
) -> AppResult<MaterialWriteSummary> {
    let mut summary = MaterialWriteSummary {
        references: BTreeMap::new(),
        inserted_materials: 0,
        reused_materials: 0,
    };
    maybe_fault(fault, EnvironmentCommitFaultPoint::BeforeCertificateInsert)?;
    for record in certificates {
        let reference = format!("certificate:{}", hex(&record.fingerprint));
        let metadata = json!({
            "fingerprint": hex(&record.fingerprint),
            "revision": certificate_inventory.checked_add(1).ok_or_else(commit_failed)?,
        });
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO certificate_material(kind, protected_blob, metadata_json, updated_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![reference, &*record.protected_payload, metadata.to_string(), Utc::now().to_rfc3339()],
        ).map_err(|_| commit_failed())?;
        classify_write(
            changed,
            &mut summary.inserted_materials,
            &mut summary.reused_materials,
        );
        summary
            .references
            .insert(record.alias.as_str().to_owned(), reference);
    }
    maybe_fault(fault, EnvironmentCommitFaultPoint::BeforeSecretInsert)?;
    for record in secrets {
        let reference = format!("secret:{}", hex(&record.fingerprint));
        let changed = transaction.execute(
            "INSERT OR IGNORE INTO protected_secrets(provider, secret_key, protected_blob, updated_at)
             VALUES ('system', ?1, ?2, ?3)",
            params![reference, &*record.protected_payload, Utc::now().to_rfc3339()],
        ).map_err(|_| commit_failed())?;
        classify_write(
            changed,
            &mut summary.inserted_materials,
            &mut summary.reused_materials,
        );
        summary
            .references
            .insert(record.alias.as_str().to_owned(), reference);
    }
    Ok(summary)
}

fn write_workspace(
    transaction: &Transaction<'_>,
    target: EnvironmentCommitTarget,
    mut workspace: ProxyWorkspace,
) -> AppResult<WorkspaceWrite> {
    match target {
        EnvironmentCommitTarget::Existing {
            workspace_id,
            expected_revision,
        } => {
            let revision = expected_revision.checked_add(1).ok_or_else(commit_failed)?;
            workspace.id = WorkspaceId::from_uuid(workspace_id);
            workspace.revision = Revision::new(revision);
            workspace.validate().map_err(AppError::from)?;
            let encoded = encode_workspace_record(&workspace)
                .map_err(|_| commit_failed())?
                .to_string();
            let changed = transaction
                .execute(
                    "UPDATE workspaces SET revision = ?1, json = ?2, updated_at = ?3
                 WHERE id = ?4 AND revision = ?5",
                    params![
                        revision_to_i64(revision).map_err(|_| commit_failed())?,
                        encoded,
                        Utc::now().to_rfc3339(),
                        workspace_id.to_string(),
                        revision_to_i64(expected_revision).map_err(|_| commit_failed())?
                    ],
                )
                .map_err(|_| commit_failed())?;
            if changed != 1 {
                return Err(baseline_mismatch());
            }
            Ok(WorkspaceWrite {
                workspace_id,
                revision,
                preserve_existing_selection: true,
            })
        }
        EnvironmentCommitTarget::New {
            workspace_id,
            display_name,
        } => {
            let new_workspace_revision = 1_u64;
            workspace.id = WorkspaceId::from_uuid(workspace_id);
            workspace.revision = Revision::new(new_workspace_revision);
            workspace.name = display_name;
            workspace.validate().map_err(AppError::from)?;
            let encoded = encode_workspace_record(&workspace)
                .map_err(|_| commit_failed())?
                .to_string();
            transaction.execute(
                "INSERT INTO workspaces(id, revision, json, updated_at) VALUES (?1, ?2, ?3, ?4)",
                params![workspace_id.to_string(), revision_to_i64(new_workspace_revision).map_err(|_| commit_failed())?, encoded, Utc::now().to_rfc3339()],
            ).map_err(|_| baseline_mismatch())?;
            Ok(WorkspaceWrite {
                workspace_id,
                revision: new_workspace_revision,
                preserve_existing_selection: false,
            })
        }
    }
}

fn apply_selection(
    transaction: &Transaction<'_>,
    policy: EnvironmentSelectionPolicy,
    workspace: &WorkspaceWrite,
) -> AppResult<()> {
    if matches!(
        policy,
        EnvironmentSelectionPolicy::PreserveExistingSelectionOrSelectNewWhenNone
    ) && !workspace.preserve_existing_selection
    {
        transaction.execute(
            "UPDATE workspace_state SET selected_id = ?1 WHERE singleton_id = 1 AND selected_id IS NULL",
            [workspace.workspace_id.to_string()],
        ).map_err(|_| commit_failed())?;
    }
    Ok(())
}

fn advance_certificate_inventory(
    transaction: &Transaction<'_>,
    references: &BTreeMap<String, String>,
) -> AppResult<()> {
    if !prepared_certificate_handles_is_empty(references) {
        transaction
            .execute(
                "UPDATE certificate_state SET revision = revision + 1 WHERE singleton_id = 1",
                [],
            )
            .map_err(|_| commit_failed())?;
    }
    Ok(())
}

fn selected_workspace_id(transaction: &Transaction<'_>) -> AppResult<Option<Uuid>> {
    transaction
        .query_row(
            "SELECT selected_id FROM workspace_state WHERE singleton_id = 1",
            [],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map_err(|_| commit_failed())?
        .flatten()
        .map(|value| Uuid::parse_str(&value).map_err(|_| commit_failed()))
        .transpose()
}

fn rewrite_material_aliases(
    mut workspace: ProxyWorkspace,
    references: &BTreeMap<String, String>,
) -> AppResult<ProxyWorkspace> {
    for reference in &mut workspace.certificate_references {
        if let Some(resolved) = references.get(&reference.reference) {
            if !resolved.starts_with("certificate:") {
                return Err(material_alias_type_mismatch());
            }
            reference.reference.clone_from(resolved);
        }
    }
    for listener in &mut workspace.listeners {
        if let ListenerDataPlane::Http(settings) = &mut listener.data_plane
            && let ForwardProxyAuthentication::Basic { credential } = &mut settings.authentication
            && let Some(resolved) = references.get(&credential.key)
        {
            if !resolved.starts_with("secret:") {
                return Err(material_alias_type_mismatch());
            }
            "system".clone_into(&mut credential.provider);
            credential.key.clone_from(resolved);
        }
    }
    workspace.validate().map_err(AppError::from)?;
    Ok(workspace)
}

fn reject_cross_family_aliases(
    certificates: &[PreparedMaterialRecord],
    secrets: &[PreparedMaterialRecord],
) -> AppResult<()> {
    if certificates.iter().any(|certificate| {
        secrets
            .iter()
            .any(|secret| secret.alias == certificate.alias)
    }) {
        return Err(AppError::new(
            "MATERIAL_ALIAS_DUPLICATE",
            "证书与秘密材料不能使用同一个别名。",
        ));
    }
    Ok(())
}

fn material_alias_type_mismatch() -> AppError {
    AppError::new(
        "MATERIAL_ALIAS_TYPE_MISMATCH",
        "材料别名的类型与 Workspace 引用位置不匹配。",
    )
}

fn maybe_fault(
    configured: Option<EnvironmentCommitFaultPoint>,
    current: EnvironmentCommitFaultPoint,
) -> AppResult<()> {
    if configured == Some(current) {
        Err(commit_rolled_back())
    } else {
        Ok(())
    }
}

fn classify_write(changed: usize, inserted: &mut usize, reused: &mut usize) {
    if changed == 0 {
        *reused += 1;
    } else {
        *inserted += 1;
    }
}

fn prepared_certificate_handles_is_empty(references: &BTreeMap<String, String>) -> bool {
    references
        .values()
        .all(|reference| !reference.starts_with("certificate:"))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(output, "{byte:02x}").expect("writing hexadecimal bytes to String cannot fail");
        output
    })
}

fn baseline_mismatch() -> AppError {
    AppError::new(
        "COMMIT_BASELINE_MISMATCH",
        "环境配置基线已经变化，提交已取消。",
    )
}

fn commit_rolled_back() -> AppError {
    AppError::new("COMMIT_ROLLED_BACK", "环境配置提交已回滚。")
}

fn rolled_back_failed(error: AppError) -> EnvironmentCommitFailure {
    EnvironmentCommitFailure::rolled_back(error, EnvironmentCommitRollbackOutcome::Failed)
}

fn commit_failed() -> AppError {
    AppError::new("COMMIT_FAILED", "环境配置提交失败。")
}
