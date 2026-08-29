use super::{
    Connection, DateTime, InfrastructureError, Mutex, OptionalExtension, Path,
    ProtectedSecretRecord, SqliteStore, StoredSettings, TransactionBehavior, Utc,
    create_current_schema, initialize_singleton_state, params, parse_settings_row,
    stored_certificate_revision,
};
use crate::sqlite::schema::{COMPATIBILITY_BASELINE_SCHEMA_VERSION, CURRENT_SCHEMA_VERSION};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExistingSchema {
    Current,
    PreCompatibilityBaseline,
}

impl SqliteStore {
    pub fn open(path: &Path) -> Result<Self, InfrastructureError> {
        let connection =
            Connection::open(path).map_err(|source| InfrastructureError::Database { source })?;
        Self::from_connection(connection)
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
        let database_is_empty = database_is_empty(&connection)?;
        let existing_schema = if database_is_empty {
            None
        } else {
            Some(classify_existing_schema(&connection)?)
        };
        connection
            .execute_batch("PRAGMA journal_mode = WAL;")
            .map_err(|source| InfrastructureError::Database { source })?;
        let store = Self {
            connection: Mutex::new(connection),
            blocking_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        match existing_schema {
            None => store.create_schema()?,
            Some(ExistingSchema::PreCompatibilityBaseline) => {
                store.reset_pre_compatibility_schema()?;
            }
            Some(ExistingSchema::Current) => {}
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
            ExistingSchema::PreCompatibilityBaseline => {
                transaction
                    .rollback()
                    .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
                reset_pre_compatibility_schema(&mut connection)
            }
        }
    }

    fn reset_pre_compatibility_schema(&self) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        reset_pre_compatibility_schema(&mut connection)
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

fn reset_pre_compatibility_schema(connection: &mut Connection) -> Result<(), InfrastructureError> {
    reset_pre_compatibility_schema_with(connection, |_| Ok(()))
}

fn reset_pre_compatibility_schema_with(
    connection: &mut Connection,
    before_initialize: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<(), InfrastructureError>,
) -> Result<(), InfrastructureError> {
    connection
        .execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    let reset = (|| {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        match classify_existing_schema(&transaction)? {
            ExistingSchema::Current => {}
            ExistingSchema::PreCompatibilityBaseline => {
                drop_pre_compatibility_objects(&transaction)?;
                before_initialize(&transaction)?;
                initialize_current_schema(&transaction)?;
            }
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::DatabaseSchema { source })
    })();
    let foreign_keys = connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|source| InfrastructureError::DatabaseSchema { source });
    reset.and(foreign_keys)
}

fn drop_pre_compatibility_objects(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), InfrastructureError> {
    let objects = {
        let mut statement = transaction
            .prepare(
                "SELECT type, name FROM sqlite_schema
                 WHERE name NOT LIKE 'sqlite_%'
                   AND type IN ('trigger', 'view', 'table')
                 ORDER BY CASE type
                     WHEN 'trigger' THEN 0
                     WHEN 'view' THEN 1
                     ELSE 2
                 END, name",
            )
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .and_then(Iterator::collect::<Result<Vec<_>, _>>)
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?
    };
    for (object_type, name) in objects {
        let object_type = match object_type.as_str() {
            "trigger" => "TRIGGER",
            "view" => "VIEW",
            "table" => "TABLE",
            _ => unreachable!("query restricts SQLite object types"),
        };
        let quoted_name = name.replace('"', "\"\"");
        transaction
            .execute_batch(&format!("DROP {object_type} IF EXISTS \"{quoted_name}\";"))
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    }
    Ok(())
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
        [(1, version)] if *version < COMPATIBILITY_BASELINE_SCHEMA_VERSION => {
            Ok(ExistingSchema::PreCompatibilityBaseline)
        }
        _ => Err(InfrastructureError::DatabaseSchemaInvalid {
            current: CURRENT_SCHEMA_VERSION,
            found: markers,
        }),
    }
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
