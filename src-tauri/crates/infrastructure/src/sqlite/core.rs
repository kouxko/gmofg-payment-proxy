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
            capture_coordination:
                super::socket_capture_coordination::SocketCaptureCoordination::default(),
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
        super::schema::migrate_workspaces_to_v5(&transaction)?;
        super::socket_captures::create_schema(&transaction)?;
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

fn update_length_prefixed(context: &mut ring::digest::Context, bytes: &[u8]) {
    context.update(&(bytes.len() as u64).to_le_bytes());
    context.update(bytes);
}
