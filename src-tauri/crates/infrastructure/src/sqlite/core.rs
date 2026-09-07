use std::{ffi::OsString, path::PathBuf};

use super::{
    Connection, DateTime, InfrastructureError, Mutex, OptionalExtension, Path,
    ProtectedSecretRecord, SqliteStore, StoredSettings, TransactionBehavior, Utc,
    create_current_schema, initialize_singleton_state, params, parse_settings_row,
    stored_certificate_revision,
};
use crate::sqlite::schema::CURRENT_SCHEMA_VERSION;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingSchema {
    Current,
    PreBaseline(i64),
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, InfrastructureError> {
        let _startup_ownership = acquire_startup_ownership(path)?;
        let connection =
            Connection::open(path).map_err(|source| InfrastructureError::Database { source })?;
        match inspect_existing_schema(&connection)? {
            None | Some(ExistingSchema::Current) => Self::from_connection(connection),
            Some(ExistingSchema::PreBaseline(_)) => {
                connection
                    .close()
                    .map_err(|(_, source)| InfrastructureError::Database { source })?;
                let connection = Connection::open(path)
                    .map_err(|source| InfrastructureError::Database { source })?;
                match inspect_existing_schema(&connection)? {
                    None | Some(ExistingSchema::Current) => Self::from_connection(connection),
                    Some(ExistingSchema::PreBaseline(_)) => {
                        connection
                            .close()
                            .map_err(|(_, source)| InfrastructureError::Database { source })?;
                        clear_pre_baseline_database(path)?;
                        let connection = Connection::open(path)
                            .map_err(|source| InfrastructureError::Database { source })?;
                        Self::from_connection(connection)
                    }
                }
            }
        }
    }

    pub fn in_memory() -> Result<Self, InfrastructureError> {
        let connection = Connection::open_in_memory()
            .map_err(|source| InfrastructureError::Database { source })?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, InfrastructureError> {
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        let existing_schema = inspect_existing_schema(&connection)?;
        if let Some(ExistingSchema::PreBaseline(version)) = existing_schema {
            return Err(InfrastructureError::DatabaseSchemaInvalid {
                current: CURRENT_SCHEMA_VERSION,
                found: vec![(1, version)],
            });
        }
        connection
            .execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|source| InfrastructureError::Database { source })?;
        let store = Self {
            connection: Mutex::new(connection),
            blocking_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        if existing_schema.is_none() {
            store.create_schema()?;
        }
        Ok(store)
    }

    fn create_schema(&self) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        if database_is_empty(&transaction)? {
            initialize_current_schema(&transaction)?;
            return transaction
                .commit()
                .map_err(|source| InfrastructureError::DatabaseSchema { source });
        }
        match classify_existing_schema(&transaction)? {
            ExistingSchema::Current => transaction
                .commit()
                .map_err(|source| InfrastructureError::DatabaseSchema { source }),
            ExistingSchema::PreBaseline(version) => {
                Err(InfrastructureError::DatabaseSchemaInvalid {
                    current: CURRENT_SCHEMA_VERSION,
                    found: vec![(1, version)],
                })
            }
        }
    }

    pub fn load_settings(&self) -> Result<Option<StoredSettings>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT revision, json, updated_at FROM settings WHERE singleton_id = 1",
                [],
                |row| {
                    let revision: i64 = row.get(0)?;
                    let json: String = row.get(1)?;
                    let updated_at: String = row.get(2)?;
                    Ok((revision, json, updated_at))
                },
            )
            .optional()
            .map_err(|source| InfrastructureError::Database { source })?
            .map(parse_settings_row)
            .transpose()
    }

    pub fn save_protected_secret(
        &self,
        record: &ProtectedSecretRecord,
    ) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "INSERT INTO protected_secrets(provider, secret_key, protected_blob, updated_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(provider, secret_key) DO UPDATE SET
                   protected_blob = excluded.protected_blob,
                   updated_at = excluded.updated_at",
                params![
                    record.provider,
                    record.key,
                    record.protected_blob,
                    record.updated_at.to_rfc3339(),
                ],
            )
            .map(|_| ())
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn load_protected_secret(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<Option<ProtectedSecretRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT protected_blob, updated_at FROM protected_secrets
                 WHERE provider = ?1 AND secret_key = ?2",
                params![provider, key],
                |row| {
                    let protected_blob = row.get(0)?;
                    let updated_at: String = row.get(1)?;
                    Ok((protected_blob, updated_at))
                },
            )
            .optional()
            .map_err(|source| InfrastructureError::Database { source })?
            .map(|(protected_blob, updated_at)| {
                Ok(ProtectedSecretRecord {
                    provider: provider.to_owned(),
                    key: key.to_owned(),
                    protected_blob,
                    updated_at: DateTime::parse_from_rfc3339(&updated_at)
                        .map(|value| value.with_timezone(&Utc))
                        .map_err(|error| InfrastructureError::PersistenceCorrupt {
                            entity: "protected_secrets",
                            message: format!("updated_at 无效：{error}"),
                        })?,
                })
            })
            .transpose()
    }

    pub fn delete_protected_secret(
        &self,
        provider: &str,
        key: &str,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "DELETE FROM protected_secrets WHERE provider = ?1 AND secret_key = ?2",
                params![provider, key],
            )
            .map(|deleted| deleted > 0)
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn protected_secret_fingerprint(
        &self,
        provider: &str,
    ) -> Result<[u8; 32], InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare(
                "SELECT secret_key, protected_blob, updated_at FROM protected_secrets
                 WHERE provider = ?1 ORDER BY secret_key",
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        let rows = statement
            .query_map([provider], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|source| InfrastructureError::Database { source })?;
        let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
        for row in rows {
            let (key, protected_blob, updated_at) =
                row.map_err(|source| InfrastructureError::Database { source })?;
            update_length_prefixed(&mut digest, key.as_bytes());
            update_length_prefixed(&mut digest, &protected_blob);
            update_length_prefixed(&mut digest, updated_at.as_bytes());
        }
        let bytes = digest.finish();
        let mut fingerprint = [0_u8; 32];
        fingerprint.copy_from_slice(bytes.as_ref());
        Ok(fingerprint)
    }
}

fn acquire_startup_ownership(path: &Path) -> Result<Connection, InfrastructureError> {
    let connection = Connection::open(sqlite_sidecar_path(path, ".startup-lock"))
        .map_err(|source| InfrastructureError::Database { source })?;
    connection
        .execute_batch("PRAGMA busy_timeout = 5000;")
        .map_err(|source| InfrastructureError::Database { source })?;
    #[cfg(test)]
    if STARTUP_OWNERSHIP_CONTENTION_PROBE_ENABLED.load(std::sync::atomic::Ordering::SeqCst) {
        connection
            .busy_handler(Some(startup_ownership_contention_probe))
            .map_err(|source| InfrastructureError::Database { source })?;
    }
    connection
        .execute_batch("BEGIN EXCLUSIVE;")
        .map_err(|source| InfrastructureError::Database { source })?;
    Ok(connection)
}

#[cfg(test)]
static STARTUP_OWNERSHIP_CONTENTION_PROBE_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
static STARTUP_OWNERSHIP_CONTENTION_OBSERVED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(test)]
fn startup_ownership_contention_probe(attempt: i32) -> bool {
    STARTUP_OWNERSHIP_CONTENTION_OBSERVED.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::sleep(std::time::Duration::from_millis(1));
    attempt < 5_000
}

fn inspect_existing_schema(
    connection: &Connection,
) -> Result<Option<ExistingSchema>, InfrastructureError> {
    if database_is_empty(connection)? {
        Ok(None)
    } else {
        classify_existing_schema(connection).map(Some)
    }
}

fn database_is_empty(connection: &Connection) -> Result<bool, InfrastructureError> {
    connection
        .query_row(
            "SELECT NOT EXISTS(
                SELECT 1 FROM sqlite_schema
                WHERE name NOT LIKE 'sqlite_%'
            )",
            [],
            |row| row.get(0),
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })
}

fn classify_existing_schema(
    connection: &Connection,
) -> Result<ExistingSchema, InfrastructureError> {
    let marker_exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'application_schema'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    if !marker_exists {
        return Err(InfrastructureError::DatabaseSchemaInvalid {
            current: CURRENT_SCHEMA_VERSION,
            found: Vec::new(),
        });
    }

    let markers = connection
        .prepare("SELECT singleton_id, version FROM application_schema ORDER BY singleton_id")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    match markers.as_slice() {
        [(1, version)] if *version == CURRENT_SCHEMA_VERSION => Ok(ExistingSchema::Current),
        [(1, version)] if *version < CURRENT_SCHEMA_VERSION => {
            Ok(ExistingSchema::PreBaseline(*version))
        }
        _ => Err(InfrastructureError::DatabaseSchemaInvalid {
            current: CURRENT_SCHEMA_VERSION,
            found: markers,
        }),
    }
}

fn clear_pre_baseline_database(path: &Path) -> Result<(), InfrastructureError> {
    for sidecar in [
        sqlite_sidecar_path(path, "-shm"),
        sqlite_sidecar_path(path, "-wal"),
    ] {
        match std::fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(InfrastructureError::DatabaseReset {
                    path: sidecar,
                    source,
                });
            }
        }
    }
    std::fs::remove_file(path).map_err(|source| InfrastructureError::DatabaseReset {
        path: path.to_path_buf(),
        source,
    })
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut value = OsString::from(path.as_os_str());
    value.push(suffix);
    PathBuf::from(value)
}

fn initialize_current_schema(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), InfrastructureError> {
    create_current_schema(transaction)?;
    let certificate_revision = stored_certificate_revision(transaction)?;
    initialize_singleton_state(transaction, certificate_revision)
}

fn update_length_prefixed(context: &mut ring::digest::Context, bytes: &[u8]) {
    context.update(&(bytes.len() as u64).to_le_bytes());
    context.update(bytes);
}

#[cfg(test)]
mod tests;
