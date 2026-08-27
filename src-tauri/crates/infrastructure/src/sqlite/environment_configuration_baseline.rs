use intercept_proxy_application::{
    AppError, AppResult, EnvironmentApplyGenerations, EnvironmentCommitTarget,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use uuid::Uuid;

use super::{SqliteStore, revision_to_i64};
use crate::{WorkspaceRecord, adapters::common::decode_workspace_record};

impl SqliteStore {
    pub(crate) fn observe_environment_apply_generations(
        &self,
        workspace_id: Uuid,
    ) -> AppResult<EnvironmentApplyGenerations> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Deferred)
            .map_err(|_| commit_failed())?;
        let selected_workspace_id = transaction
            .query_row(
                "SELECT selected_id FROM workspace_state WHERE singleton_id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|_| commit_failed())?
            .flatten()
            .map(|value| Uuid::parse_str(&value).map_err(|_| commit_failed()))
            .transpose()?;
        let application_mutation = transaction
            .query_row(
                "SELECT json FROM workspaces WHERE id = ?1",
                [workspace_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| commit_failed())?
            .map_or(0, |json| stable_digest_u64([json.as_bytes()]));
        let certificate_revision = transaction
            .query_row(
                "SELECT revision FROM certificate_state WHERE singleton_id = 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|_| commit_failed())?;
        let certificate_revision =
            u64::try_from(certificate_revision).map_err(|_| commit_failed())?;
        let package_inventory = exact_package_inventory(&transaction)?;
        Ok(EnvironmentApplyGenerations {
            selected_workspace_id,
            listener: 0,
            android: 0,
            package: 0,
            package_inventory,
            certificate_inventory: exact_certificate_inventory(&transaction, certificate_revision)?,
            protected_secret_inventory: exact_secret_inventory(&transaction)?,
            application_mutation,
        })
    }
}

pub(super) fn check_workspace_baseline(
    transaction: &Transaction<'_>,
    target: &EnvironmentCommitTarget,
    baseline: &EnvironmentApplyGenerations,
) -> AppResult<()> {
    let EnvironmentCommitTarget::Existing {
        workspace_id,
        expected_revision,
    } = target
    else {
        return Ok(());
    };
    let json = transaction
        .query_row(
            "SELECT json FROM workspaces WHERE id = ?1 AND revision = ?2",
            params![
                workspace_id.to_string(),
                revision_to_i64(*expected_revision).map_err(|_| commit_failed())?
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| commit_failed())?
        .ok_or_else(baseline_mismatch)?;
    let value = serde_json::from_str(&json).map_err(|_| baseline_mismatch())?;
    decode_workspace_record(WorkspaceRecord {
        id: *workspace_id,
        revision: *expected_revision,
        value,
        updated_at: chrono::Utc::now(),
    })
    .map_err(|_| baseline_mismatch())?;
    if baseline.application_mutation != 0
        && stable_digest_u64([json.as_bytes()]) != baseline.application_mutation
    {
        return Err(baseline_mismatch());
    }
    Ok(())
}

pub(super) fn exact_package_inventory(transaction: &Transaction<'_>) -> AppResult<u64> {
    let mut statement = transaction
        .prepare(
            "SELECT package_id, version, name, generation, enabled
             FROM protocol_packages ORDER BY package_id, version",
        )
        .map_err(|_| commit_failed())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(|_| commit_failed())?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut count = 0_u64;
    for row in rows {
        let (id, version, name, generation, enabled) = row.map_err(|_| commit_failed())?;
        update_framed(&mut context, id.as_bytes());
        update_framed(&mut context, version.as_bytes());
        update_framed(&mut context, name.as_bytes());
        update_framed(&mut context, generation.as_bytes());
        update_framed(&mut context, &enabled.to_be_bytes());
        count += 1;
    }
    Ok(finish_inventory(context, count))
}

pub(super) fn exact_certificate_inventory(
    transaction: &Transaction<'_>,
    revision: u64,
) -> AppResult<u64> {
    let mut statement = transaction
        .prepare(
            "SELECT kind, protected_blob, metadata_json
             FROM certificate_material ORDER BY kind",
        )
        .map_err(|_| commit_failed())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| commit_failed())?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    update_framed(&mut context, &revision.to_be_bytes());
    let mut count = 0_u64;
    for row in rows {
        let (kind, protected_blob, metadata) = row.map_err(|_| commit_failed())?;
        update_framed(&mut context, kind.as_bytes());
        update_framed(&mut context, &protected_blob);
        update_framed(&mut context, metadata.as_bytes());
        count += 1;
    }
    if count == 0 {
        Ok(revision)
    } else {
        Ok(finish_inventory(context, count))
    }
}

pub(super) fn exact_secret_inventory(transaction: &Transaction<'_>) -> AppResult<u64> {
    let mut statement = transaction
        .prepare(
            "SELECT provider, secret_key, protected_blob
             FROM protected_secrets ORDER BY provider, secret_key",
        )
        .map_err(|_| commit_failed())?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })
        .map_err(|_| commit_failed())?;
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    let mut count = 0_u64;
    for row in rows {
        let (provider, key, protected_blob) = row.map_err(|_| commit_failed())?;
        update_framed(&mut context, provider.as_bytes());
        update_framed(&mut context, key.as_bytes());
        update_framed(&mut context, &protected_blob);
        count += 1;
    }
    Ok(finish_inventory(context, count))
}

fn update_framed(context: &mut ring::digest::Context, bytes: &[u8]) {
    context.update(&(bytes.len() as u64).to_be_bytes());
    context.update(bytes);
}

fn finish_inventory(context: ring::digest::Context, count: u64) -> u64 {
    if count == 0 {
        return 0;
    }
    let digest = context.finish();
    u64::from_be_bytes(
        digest.as_ref()[..8]
            .try_into()
            .expect("SHA-256 prefix has exactly eight bytes"),
    )
}

fn stable_digest_u64<'bytes>(parts: impl IntoIterator<Item = &'bytes [u8]>) -> u64 {
    let mut context = ring::digest::Context::new(&ring::digest::SHA256);
    for part in parts {
        update_framed(&mut context, part);
    }
    finish_inventory(context, 1)
}

fn baseline_mismatch() -> AppError {
    AppError::new(
        "COMMIT_BASELINE_MISMATCH",
        "环境配置基线已经变化，提交已取消。",
    )
}

fn commit_failed() -> AppError {
    AppError::new("COMMIT_FAILED", "环境配置提交失败。")
}
