use super::{
    Connection, DateTime, InfrastructureError, Mutex, OptionalExtension, Path,
    ProtectedSecretRecord, SqliteStore, StoredSettings, Utc, create_current_schema,
    initialize_singleton_state, params, parse_settings_row, stored_certificate_revision,
};
use crate::sqlite::schema::CURRENT_SCHEMA_VERSION;

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
                 PRAGMA busy_timeout = 5000;
                 PRAGMA journal_mode = WAL;",
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        let store = Self {
            connection: Mutex::new(connection),
            blocking_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        };
        store.ensure_current_schema()?;
        Ok(store)
    }

    fn ensure_current_schema(&self) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let database_is_empty = database_is_empty(&connection)?;
        let reset_required = !database_is_empty
            && validate_current_schema_marker(&connection)? == SchemaState::Older;

        if reset_required {
            connection
                .execute_batch("PRAGMA foreign_keys = OFF;")
                .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        }

        let result = (|| {
            let transaction = connection
                .transaction()
                .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
            if reset_required {
                drop_all_user_tables(&transaction)?;
            }
            if database_is_empty || reset_required {
                create_current_schema(&transaction)?;
                let certificate_revision = stored_certificate_revision(&transaction)?;
                initialize_singleton_state(&transaction, certificate_revision)?;
            }
            migrate_workspace_documents(&transaction)?;
            transaction
                .commit()
                .map_err(|source| InfrastructureError::DatabaseSchema { source })
        })();

        if reset_required {
            connection
                .execute_batch("PRAGMA foreign_keys = ON;")
                .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
        }
        result
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

fn migrate_workspace_documents(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), InfrastructureError> {
    const PREVIOUS_WORKSPACE_VERSION: u64 = 6;
    let rows = transaction
        .prepare("SELECT id, revision, json FROM workspaces ORDER BY id")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    for (id, indexed_revision, json) in rows {
        let mut value = serde_json::from_str::<serde_json::Value>(&json).map_err(|error| {
            InfrastructureError::PersistenceCorrupt {
                entity: "workspaces",
                message: format!("Workspace {id} JSON 无效：{error}"),
            }
        })?;
        let Some(object) = value.as_object_mut() else {
            return Err(InfrastructureError::PersistenceCorrupt {
                entity: "workspaces",
                message: format!("Workspace {id} 必须是 JSON object"),
            });
        };
        if object
            .get("_persistence_version")
            .and_then(serde_json::Value::as_u64)
            != Some(PREVIOUS_WORKSPACE_VERSION)
        {
            continue;
        }
        let revision = object
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| InfrastructureError::PersistenceCorrupt {
                entity: "workspaces",
                message: format!("Workspace {id} revision 无效"),
            })?;
        if i64::try_from(revision).ok() != Some(indexed_revision) {
            return Err(InfrastructureError::PersistenceCorrupt {
                entity: "workspaces",
                message: format!("Workspace {id} 索引 revision 与 JSON 不一致"),
            });
        }
        let rules = object
            .get_mut("rules")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or_else(|| InfrastructureError::PersistenceCorrupt {
                entity: "workspaces",
                message: format!("Workspace {id} v6 rules 必须是数组"),
            })?;
        for rule in rules.iter() {
            let channel = rule
                .as_object()
                .and_then(|rule| rule.get("channel"))
                .ok_or_else(|| InfrastructureError::PersistenceCorrupt {
                    entity: "workspaces",
                    message: format!("Workspace {id} v6 普通规则缺少 channel"),
                })?;
            if !channel.is_null() && !channel.is_string() {
                return Err(InfrastructureError::PersistenceCorrupt {
                    entity: "workspaces",
                    message: format!("Workspace {id} v6 普通规则 channel 无效"),
                });
            }
        }
        rules.retain(|rule| !rule["channel"].is_null());
        let next_revision = revision
            .checked_add(1)
            .ok_or(InfrastructureError::RevisionConflict)?;
        let next_indexed_revision =
            i64::try_from(next_revision).map_err(|_| InfrastructureError::RevisionConflict)?;
        object.insert("revision".into(), serde_json::json!(next_revision));
        object.insert(
            "_persistence_version".into(),
            serde_json::json!(intercept_proxy_application::WORKSPACE_PERSISTENCE_VERSION),
        );
        let changed = transaction
            .execute(
                "UPDATE workspaces SET revision = ?1, json = ?2, updated_at = ?3 WHERE id = ?4 AND revision = ?5",
                params![next_indexed_revision, value.to_string(), Utc::now().to_rfc3339(), id, indexed_revision],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if changed != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
    }
    Ok(())
}

fn database_is_empty(connection: &Connection) -> Result<bool, InfrastructureError> {
    connection
        .query_row(
            "SELECT NOT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
            )",
            [],
            |row| row.get(0),
        )
        .map_err(|source| InfrastructureError::DatabaseSchema { source })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SchemaState {
    Current,
    Older,
}

fn validate_current_schema_marker(
    connection: &Connection,
) -> Result<SchemaState, InfrastructureError> {
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
        return Ok(SchemaState::Older);
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
        [(1, version)] if *version == CURRENT_SCHEMA_VERSION => Ok(SchemaState::Current),
        [(1, version)] if *version < CURRENT_SCHEMA_VERSION => Ok(SchemaState::Older),
        _ => Err(InfrastructureError::DatabaseSchemaInvalid {
            current: CURRENT_SCHEMA_VERSION,
            found: markers,
        }),
    }
}

fn drop_all_user_tables(
    transaction: &rusqlite::Transaction<'_>,
) -> Result<(), InfrastructureError> {
    let tables = transaction
        .prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
             ORDER BY name",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        transaction
            .execute_batch(&format!("DROP TABLE \"{quoted}\";"))
            .map_err(|source| InfrastructureError::DatabaseSchema { source })?;
    }
    Ok(())
}

fn update_length_prefixed(context: &mut ring::digest::Context, bytes: &[u8]) {
    context.update(&(bytes.len() as u64).to_le_bytes());
    context.update(bytes);
}
