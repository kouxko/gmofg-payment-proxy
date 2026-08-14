use super::{
    Connection, DateTime, InfrastructureError, Mutex, OptionalExtension, Path,
    ProtectedSecretRecord, SqliteStore, StoredSettings, Utc, create_schema,
    initialize_singleton_state, params, parse_settings_row, record_schema_migration,
    stored_certificate_revision,
};

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
        };
        store.migrate()?;
        Ok(store)
    }

    pub(super) fn migrate(&self) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::DatabaseMigration { source })?;
        create_schema(&transaction)?;
        let certificate_revision = stored_certificate_revision(&transaction)?;
        initialize_singleton_state(&transaction, certificate_revision)?;
        record_schema_migration(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::DatabaseMigration { source })
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
}
