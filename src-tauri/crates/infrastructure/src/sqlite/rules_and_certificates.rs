use super::{
    CertificateMaterialRecord, CertificateMaterialSnapshot, InfrastructureError, OptionalExtension,
    RuleCollectionSnapshot, RuleRecord, RuleRuntimeUpdate, SqliteStore, TransactionBehavior, Utc,
    Uuid, Value, advance_rule_collection_revision, current_revision, insert_rule,
    load_certificate_material, load_rule_records, params, put_certificate_material,
    revision_to_i64, rule_record_corrupt, rule_signature,
};

impl SqliteStore {
    pub fn list_rules(&self) -> Result<Vec<RuleRecord>, InfrastructureError> {
        Ok(self.load_rules_snapshot()?.records)
    }

    pub fn load_rules_snapshot(&self) -> Result<RuleCollectionSnapshot, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| InfrastructureError::Database { source })?;
        let revision =
            current_revision(&transaction, "rule_state", "singleton_id = 1")?.unwrap_or(0);
        let records = load_rule_records(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(RuleCollectionSnapshot { revision, records })
    }

    pub fn insert_rule(&self, record: &RuleRecord) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = insert_rule(&transaction, record)?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn compare_and_swap_rule(
        &self,
        expected_revision: u64,
        record: &RuleRecord,
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = transaction
            .execute(
                "UPDATE rules
                 SET revision = ?1, enabled = ?2, json = ?3, updated_at = ?4
                 WHERE id = ?5 AND revision = ?6",
                params![
                    revision_to_i64(record.revision)?,
                    record.enabled,
                    record.value.to_string(),
                    record.updated_at.to_rfc3339(),
                    record.id.to_string(),
                    revision_to_i64(expected_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn delete_rule(&self, id: Uuid, expected_revision: u64) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let affected = transaction
            .execute(
                "DELETE FROM rules WHERE id = ?1 AND revision = ?2",
                params![id.to_string(), revision_to_i64(expected_revision)?],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        advance_rule_collection_revision(&transaction)?;
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })
    }

    pub fn replace_rules_atomically(
        &self,
        expected_collection_revision: u64,
        records: &[RuleRecord],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        // 整套规则先 CAS 推进集合版本，再删除/插入；任一条失败都会回滚整个事务，读者
        // 不可能看到“旧规则已删、新规则只写了一半”的中间状态。
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let next_revision = expected_collection_revision.saturating_add(1);
        let affected = transaction
            .execute(
                "UPDATE rule_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_revision)?,
                    revision_to_i64(expected_collection_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .execute("DELETE FROM rules", [])
            .map_err(|source| InfrastructureError::Database { source })?;
        for record in records {
            if insert_rule(&transaction, record)? != 1 {
                return Err(InfrastructureError::RevisionConflict);
            }
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_revision)
    }

    pub fn compare_and_swap_rule_runtime(
        &self,
        expected_collection_revision: u64,
        expected_signature: &[(Uuid, u64)],
        updates: &[RuleRuntimeUpdate],
    ) -> Result<u64, InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| InfrastructureError::Database { source })?;
        let current_collection_revision =
            current_revision(&transaction, "rule_state", "singleton_id = 1")?.unwrap_or(0);
        if current_collection_revision != expected_collection_revision {
            return Err(InfrastructureError::RevisionConflict);
        }
        let current_signature = rule_signature(&transaction)?;
        let mut expected_signature = expected_signature.to_vec();
        expected_signature.sort_unstable();
        if current_signature != expected_signature {
            return Err(InfrastructureError::RevisionConflict);
        }

        for update in updates {
            let mut value = transaction
                .query_row(
                    "SELECT json FROM rules WHERE id = ?1 AND revision = ?2",
                    params![
                        update.id.to_string(),
                        revision_to_i64(update.expected_revision)?
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|source| InfrastructureError::Database { source })?
                .ok_or(InfrastructureError::RevisionConflict)
                .and_then(|json| {
                    serde_json::from_str::<Value>(&json)
                        .map_err(|error| rule_record_corrupt(format!("JSON 无效：{error}")))
                })?;
            let object = value
                .as_object_mut()
                .ok_or_else(|| rule_record_corrupt("JSON 必须是对象"))?;
            object.insert("hit_count".into(), Value::from(update.hit_count));
            object.insert(
                "last_hit_at".into(),
                serde_json::to_value(update.last_hit_at)
                    .map_err(|error| rule_record_corrupt(format!("命中时间序列化失败：{error}")))?,
            );
            if update.revision != update.expected_revision {
                object.insert("revision".into(), Value::from(update.revision));
                object.insert("enabled".into(), Value::from(update.enabled));
            }
            let affected = transaction
                .execute(
                    "UPDATE rules
                     SET revision = ?1, enabled = ?2, json = ?3, updated_at = ?4
                     WHERE id = ?5 AND revision = ?6",
                    params![
                        revision_to_i64(update.revision)?,
                        update.enabled,
                        value.to_string(),
                        Utc::now().to_rfc3339(),
                        update.id.to_string(),
                        revision_to_i64(update.expected_revision)?
                    ],
                )
                .map_err(|source| InfrastructureError::Database { source })?;
            if affected != 1 {
                return Err(InfrastructureError::RevisionConflict);
            }
        }
        let next_collection_revision = expected_collection_revision.saturating_add(1);
        let affected = transaction
            .execute(
                "UPDATE rule_state SET revision = ?1
                 WHERE singleton_id = 1 AND revision = ?2",
                params![
                    revision_to_i64(next_collection_revision)?,
                    revision_to_i64(expected_collection_revision)?
                ],
            )
            .map_err(|source| InfrastructureError::Database { source })?;
        if affected != 1 {
            return Err(InfrastructureError::RevisionConflict);
        }
        transaction
            .commit()
            .map_err(|source| InfrastructureError::Database { source })?;
        Ok(next_collection_revision)
    }

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
