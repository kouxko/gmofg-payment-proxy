use rusqlite::Transaction;

use super::InfrastructureError;

/// 产品 1.00 的正式数据库兼容起点。
///
/// 更早的版本只属于发布前开发数据，可以在启动时清空。后续版本必须从这个基线开始
/// 提供显式兼容迁移，不能再次使用清空策略。
pub(super) const COMPATIBILITY_BASELINE_SCHEMA_VERSION: i64 = 100;
pub(super) const CURRENT_SCHEMA_VERSION: i64 = COMPATIBILITY_BASELINE_SCHEMA_VERSION;

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
            CREATE TABLE application_feature_state (
                feature_key TEXT PRIMARY KEY, initialized_at TEXT NOT NULL
            );
            CREATE TABLE android_runtime_owners (
                serial TEXT PRIMARY KEY,
                epoch TEXT NOT NULL UNIQUE,
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
            CREATE TRIGGER android_runtime_owners_capacity
            BEFORE INSERT ON android_runtime_owners
            WHEN NOT EXISTS (
                SELECT 1 FROM android_runtime_owners WHERE serial = NEW.serial
            ) AND (SELECT COUNT(*) FROM android_runtime_owners) >= 8
            BEGIN
                SELECT RAISE(ABORT, 'ANDROID_RUNTIME_CAPACITY_EXCEEDED');
            END;
            ",
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    create_external_package_schema(transaction)?;
    transaction
        .execute(
            "INSERT INTO application_schema(singleton_id, version) VALUES (1, ?1)",
            [CURRENT_SCHEMA_VERSION],
        )
        .map(|_| ())
        .map_err(|source| InfrastructureError::DatabaseSchema { source })
}

fn create_external_package_schema(
    transaction: &Transaction<'_>,
) -> Result<(), InfrastructureError> {
    transaction
        .execute_batch(
            "CREATE TABLE external_protocol_packages (
                package_id TEXT NOT NULL,
                version TEXT NOT NULL,
                registration_json TEXT NOT NULL,
                registration_fingerprint BLOB NOT NULL
                    CHECK(length(registration_fingerprint) = 32),
                local_archive BLOB NULL,
                enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                first_connected_at TEXT NOT NULL,
                last_connected_at TEXT NOT NULL,
                last_remote_address TEXT NULL,
                recent_error_code TEXT NULL,
                recent_error_message TEXT NULL,
                recent_error_occurred_at TEXT NULL,
                CHECK(
                    (recent_error_code IS NULL
                        AND recent_error_message IS NULL
                        AND recent_error_occurred_at IS NULL)
                    OR
                    (recent_error_code IS NOT NULL
                        AND recent_error_message IS NOT NULL
                        AND recent_error_occurred_at IS NOT NULL)
                ),
                PRIMARY KEY(package_id, version)
            );",
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })
}
