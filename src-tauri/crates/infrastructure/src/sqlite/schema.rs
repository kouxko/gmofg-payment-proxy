use rusqlite::{OptionalExtension, Transaction, params};

use super::{InfrastructureError, load_workspace_records};
use crate::adapters::common::{decode_workspace_record, encode_workspace_record};

const WORKSPACE_V5_SCHEMA_MIGRATION: i64 = 10;

pub(super) fn create_schema(transaction: &Transaction<'_>) -> Result<(), InfrastructureError> {
    transaction
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                revision INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rules (
                id TEXT PRIMARY KEY, revision INTEGER NOT NULL, enabled INTEGER NOT NULL,
                json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS rule_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1), revision INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS certificate_material (
                kind TEXT PRIMARY KEY, protected_blob BLOB NOT NULL,
                metadata_json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS certificate_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1), revision INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY, revision INTEGER NOT NULL,
                json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS workspace_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1), selected_id TEXT NULL,
                FOREIGN KEY(selected_id) REFERENCES workspaces(id) ON DELETE SET NULL
            );
            CREATE TABLE IF NOT EXISTS protected_secrets (
                provider TEXT NOT NULL, secret_key TEXT NOT NULL, protected_blob BLOB NOT NULL,
                updated_at TEXT NOT NULL, PRIMARY KEY(provider, secret_key)
            );
            CREATE TABLE IF NOT EXISTS protocol_packages (
                package_id TEXT NOT NULL, version TEXT NOT NULL, name TEXT NOT NULL,
                host_api INTEGER NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                validation_state TEXT NOT NULL, validation_error_code TEXT NULL,
                installed_at TEXT NOT NULL, generation TEXT NOT NULL,
                PRIMARY KEY(package_id, version), CHECK(validation_state IN ('valid', 'invalid')),
                CHECK((validation_state = 'valid' AND validation_error_code IS NULL)
                    OR (validation_state = 'invalid' AND validation_error_code IS NOT NULL))
            );
            CREATE TABLE IF NOT EXISTS protocol_package_files (
                package_id TEXT NOT NULL, version TEXT NOT NULL, path TEXT NOT NULL,
                contents BLOB NOT NULL, PRIMARY KEY(package_id, version, path),
                FOREIGN KEY(package_id, version) REFERENCES protocol_packages(package_id, version)
                    ON DELETE CASCADE
            );
            ",
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    migrate_android_runtime_owner(transaction)
}

pub(super) fn migrate_workspaces_to_v5(
    transaction: &Transaction<'_>,
) -> Result<(), InfrastructureError> {
    let already_applied = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [WORKSPACE_V5_SCHEMA_MIGRATION],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    if already_applied {
        return Ok(());
    }

    let records = load_workspace_records(transaction)?;
    let migrated = records
        .into_iter()
        .map(|record| {
            let id = record.id;
            let revision = record.revision;
            let workspace = decode_workspace_record(record)
                .map_err(|message| InfrastructureError::DatabaseMigrationInvalid { message })?;
            let value = encode_workspace_record(&workspace)
                .map_err(|message| InfrastructureError::DatabaseMigrationInvalid { message })?;
            Ok((id, revision, value))
        })
        .collect::<Result<Vec<_>, InfrastructureError>>()?;

    for (id, revision, value) in migrated {
        let changed = transaction
            .execute(
                "UPDATE workspaces SET json = ?1 WHERE id = ?2 AND revision = ?3",
                params![
                    value.to_string(),
                    id.to_string(),
                    i64::try_from(revision).map_err(|_| {
                        InfrastructureError::DatabaseMigrationInvalid {
                            message: format!("Workspace {id} revision 超出 SQLite 范围"),
                        }
                    })?,
                ],
            )
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
        if changed != 1 {
            return Err(InfrastructureError::DatabaseMigrationInvalid {
                message: format!("Workspace {id} 在迁移期间发生变化"),
            });
        }
    }
    Ok(())
}

fn migrate_android_runtime_owner(transaction: &Transaction<'_>) -> Result<(), InfrastructureError> {
    let definition = transaction
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'android_runtime_owner'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    if definition
        .as_deref()
        .is_some_and(|sql| !sql.contains("waiting_reconnect"))
    {
        transaction
            .execute_batch(
                "ALTER TABLE android_runtime_owner RENAME TO android_runtime_owner_v8;
                 CREATE TABLE android_runtime_owner (
                    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                    serial TEXT NOT NULL, epoch TEXT NOT NULL,
                    mode TEXT NOT NULL CHECK(mode IN ('device_only', 'lan', 'adb_reverse')),
                    profile_id TEXT NOT NULL,
                    state TEXT NOT NULL CHECK(state IN (
                        'active', 'uncertain', 'waiting_reconnect', 'cleanup_required', 'stop_failed'
                    )),
                    source TEXT NOT NULL CHECK(source IN ('start', 'apply', 'recovery')),
                    transition_reason TEXT NOT NULL CHECK(transition_reason IN (
                        'activation_confirmed', 'activation_uncertain', 'reverse_preparation',
                        'reverse_cleanup_required', 'device_disconnected', 'device_reconnected',
                        'stop_failed', 'recovered_from_storage'
                    )),
                    reverse_ports_json TEXT NOT NULL, resume_state TEXT NULL,
                    updated_at TEXT NOT NULL
                 );
                 INSERT INTO android_runtime_owner(
                    singleton_id, serial, epoch, mode, profile_id, state, source,
                    transition_reason, reverse_ports_json, updated_at
                 ) SELECT singleton_id, serial, epoch, mode, profile_id, state, source,
                    transition_reason, reverse_ports_json, updated_at
                 FROM android_runtime_owner_v8;
                 DROP TABLE android_runtime_owner_v8;",
            )
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    } else if definition.is_none() {
        transaction
            .execute_batch(
                "CREATE TABLE android_runtime_owner (
                    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                    serial TEXT NOT NULL, epoch TEXT NOT NULL,
                    mode TEXT NOT NULL CHECK(mode IN ('device_only', 'lan', 'adb_reverse')),
                    profile_id TEXT NOT NULL,
                    state TEXT NOT NULL CHECK(state IN (
                        'active', 'uncertain', 'waiting_reconnect', 'cleanup_required', 'stop_failed'
                    )),
                    source TEXT NOT NULL CHECK(source IN ('start', 'apply', 'recovery')),
                    transition_reason TEXT NOT NULL CHECK(transition_reason IN (
                        'activation_confirmed', 'activation_uncertain', 'reverse_preparation',
                        'reverse_cleanup_required', 'device_disconnected', 'device_reconnected',
                        'stop_failed', 'recovered_from_storage'
                    )),
                    reverse_ports_json TEXT NOT NULL, resume_state TEXT NULL,
                    updated_at TEXT NOT NULL
                 );",
            )
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
    }
    Ok(())
}
