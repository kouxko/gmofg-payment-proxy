use rusqlite::Transaction;

use super::InfrastructureError;

pub(super) const CURRENT_SCHEMA_VERSION: i64 = 15;

pub(super) fn create_current_schema(
    transaction: &Transaction<'_>,
) -> Result<(), InfrastructureError> {
    transaction
        .execute_batch(
            "
            CREATE TABLE application_schema (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                version INTEGER NOT NULL
            );
            CREATE TABLE settings (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                revision INTEGER NOT NULL, json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE certificate_material (
                kind TEXT PRIMARY KEY, protected_blob BLOB NOT NULL,
                metadata_json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE certificate_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1), revision INTEGER NOT NULL
            );
            CREATE TABLE workspaces (
                id TEXT PRIMARY KEY, revision INTEGER NOT NULL,
                json TEXT NOT NULL, updated_at TEXT NOT NULL
            );
            CREATE TABLE workspace_state (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1), selected_id TEXT NULL,
                FOREIGN KEY(selected_id) REFERENCES workspaces(id) ON DELETE SET NULL
            );
            CREATE TABLE protected_secrets (
                provider TEXT NOT NULL, secret_key TEXT NOT NULL, protected_blob BLOB NOT NULL,
                updated_at TEXT NOT NULL, PRIMARY KEY(provider, secret_key)
            );
            CREATE TABLE protocol_packages (
                package_id TEXT NOT NULL, version TEXT NOT NULL, name TEXT NOT NULL,
                host_api INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('http', 'socket')),
                enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                validation_state TEXT NOT NULL, validation_error_code TEXT NULL,
                installed_at TEXT NOT NULL, generation TEXT NOT NULL,
                PRIMARY KEY(package_id, version), CHECK(validation_state IN ('valid', 'invalid')),
                CHECK((validation_state = 'valid' AND validation_error_code IS NULL)
                    OR (validation_state = 'invalid' AND validation_error_code IS NOT NULL))
            );
            CREATE TABLE protocol_package_files (
                package_id TEXT NOT NULL, version TEXT NOT NULL, path TEXT NOT NULL,
                contents BLOB NOT NULL, PRIMARY KEY(package_id, version, path),
                FOREIGN KEY(package_id, version) REFERENCES protocol_packages(package_id, version)
                    ON DELETE CASCADE
            );
            CREATE TABLE application_feature_state (
                feature_key TEXT PRIMARY KEY, initialized_at TEXT NOT NULL
            );
            CREATE TABLE android_runtime_owner (
                singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
                serial TEXT NOT NULL, epoch TEXT NOT NULL,
                mode TEXT NOT NULL CHECK(mode IN ('device_only', 'lan', 'adb_reverse')),
                profile_id TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN (
                    'active', 'uncertain', 'waiting_reconnect', 'cleanup_required', 'stop_failed',
                    'faulted'
                )),
                source TEXT NOT NULL CHECK(source IN ('start', 'apply', 'recovery')),
                transition_reason TEXT NOT NULL CHECK(transition_reason IN (
                    'activation_confirmed', 'activation_uncertain', 'reverse_preparation',
                    'reverse_cleanup_required', 'device_disconnected', 'device_reconnected',
                    'stop_failed', 'recovered_from_storage', 'lan_endpoint_reapplied',
                    'lan_endpoint_faulted'
                )),
                reverse_ports_json TEXT NOT NULL, resume_state TEXT NULL,
                runtime_endpoints_json TEXT NOT NULL DEFAULT '[]',
                updated_at TEXT NOT NULL
            );
            ",
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    transaction
        .execute(
            "INSERT INTO application_schema(singleton_id, version) VALUES (1, ?1)",
            [CURRENT_SCHEMA_VERSION],
        )
        .map(|_| ())
        .map_err(|source| InfrastructureError::DatabaseSchema { source })
}
