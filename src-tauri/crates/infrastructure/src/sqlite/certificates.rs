use super::{
    CertificateMaterialRecord, CertificateMaterialSnapshot, InfrastructureError, SqliteStore,
    TransactionBehavior, Value, current_revision, load_certificate_material, params,
    put_certificate_material, revision_to_i64,
};

impl SqliteStore {
    pub fn compare_and_swap_certificate_materials(
        &self,
        expected_revision: u64,
        records: &[CertificateMaterialRecord],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let current_revision =
            current_revision(&transaction, "certificate_state", "singleton_id = 1")?.unwrap_or(0);
        if current_revision != expected_revision {
            return Err(InfrastructureError::RevisionConflict);
        }
        let next_revision = expected_revision.saturating_add(1);
        for record in records {
            if record.metadata.get("revision").and_then(Value::as_u64) != Some(next_revision) {
                return Err(InfrastructureError::CertificateInvalid {
                    message: "证书记录修订号与聚合修订号不一致".into(),
                });
            }
        }
        for record in records {
            put_certificate_material(&transaction, record)?;
        }
        let affected = transaction
            .execute(
                "UPDATE certificate_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_revision)?,
                    revision_to_i64(expected_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_revision)
    }

    pub fn load_certificate_materials_snapshot(
        &self,
        kinds: &[&str],
    ) -> Result<CertificateMaterialSnapshot, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let revision =
            current_revision(&transaction, "certificate_state", "singleton_id = 1")?.unwrap_or(0);
        let records = kinds
            .iter()
            .filter_map(|kind| {
                load_certificate_material(&transaction, kind)
                    .transpose()
                    .map(|result| result.map(|record| (kind, record)))
            })
            .map(|result| result.map(|(_, record)| record))
            .collect::<Result<Vec<_>, _>>()?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(CertificateMaterialSnapshot { revision, records })
    }

    pub fn load_certificate_material(
        &self,
        kind: &str,
    ) -> Result<Option<CertificateMaterialRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        load_certificate_material(&connection, kind)
    }

    #[cfg(test)]
    pub(super) fn table_names(&self) -> Result<Vec<String>, InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .map_err(|source| InfrastructureError::Database { source })?;
        statement
            .query_map([], |row| row.get(0))
            .map_err(|source| InfrastructureError::Database { source })?
            .map(|row| row.map_err(|source| InfrastructureError::Database { source }))
            .collect()
    }
}
