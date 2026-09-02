use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel,
};
use rusqlite::{TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{InfrastructureError, SqliteStore};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AndroidRuntimeOwnerRecord {
    pub owner: AndroidRuntimeOwnerViewModel,
    pub reverse_ports: Vec<u16>,
    pub resume_state: Option<AndroidRuntimeOwnerState>,
    pub runtime_endpoints: Vec<AndroidRuntimeEndpointViewModel>,
}

impl SqliteStore {
    pub fn load_android_runtime_owners(
        &self,
    ) -> Result<Vec<AndroidRuntimeOwnerRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        let mut statement = connection
            .prepare(
                "SELECT serial, epoch, mode, profile_id, state, source,
                        transition_reason, reverse_ports_json, resume_state, runtime_endpoints_json,
                        updated_at
                 FROM android_runtime_owners
                 ORDER BY serial",
            )
            .map_err(database_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })
            .map_err(database_error)?;
        rows.map(|row| row.map_err(database_error).and_then(parse_record))
            .collect()
    }

    pub fn reserve_android_runtime_owner(
        &self,
        record: &AndroidRuntimeOwnerRecord,
    ) -> Result<(), InfrastructureError> {
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        let serial_exists = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM android_runtime_owners WHERE serial = ?1)",
                [&record.owner.serial],
                |row| row.get::<_, bool>(0),
            )
            .map_err(database_error)?;
        if serial_exists {
            return Err(InfrastructureError::RevisionConflict);
        }
        let retained_count = transaction
            .query_row("SELECT COUNT(*) FROM android_runtime_owners", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(database_error)?;
        if retained_count >= 8 {
            return Err(capacity_error());
        }
        insert_record(&transaction, record)?;
        transaction.commit().map_err(database_error)
    }

    pub fn clear_android_runtime_owner(
        &self,
        serial: &str,
        expected_epoch: Uuid,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "DELETE FROM android_runtime_owners WHERE serial = ?1 AND epoch = ?2",
                params![serial, expected_epoch.to_string()],
            )
            .map(|deleted| deleted == 1)
            .map_err(database_error)
    }

    pub fn replace_android_runtime_owner_if_epoch(
        &self,
        serial: &str,
        expected_epoch: Uuid,
        record: &AndroidRuntimeOwnerRecord,
    ) -> Result<bool, InfrastructureError> {
        if record.owner.serial != serial {
            return Err(corrupt(format!(
                "替换记录 serial {} 与目标 serial {serial} 不一致",
                record.owner.serial
            )));
        }
        let connection = self.connection.lock();
        connection
            .execute(
                "UPDATE android_runtime_owners SET
                    epoch = ?1, mode = ?2, profile_id = ?3, state = ?4, source = ?5,
                    transition_reason = ?6, reverse_ports_json = ?7, resume_state = ?8,
                    runtime_endpoints_json = ?9, updated_at = ?10
                    WHERE serial = ?11 AND epoch = ?12",
                params![
                    record.owner.epoch.to_string(),
                    mode_text(record.owner.mode),
                    record.owner.profile_id,
                    state_text(record.owner.state),
                    source_text(record.owner.source),
                    reason_text(record.owner.transition_reason),
                    encode_json(&record.reverse_ports)?,
                    record.resume_state.map(state_text),
                    encode_json(&record.runtime_endpoints)?,
                    record.owner.updated_at.to_rfc3339(),
                    serial,
                    expected_epoch.to_string(),
                ],
            )
            .map(|updated| updated == 1)
            .map_err(database_error)
    }
}

fn insert_record(
    transaction: &rusqlite::Transaction<'_>,
    record: &AndroidRuntimeOwnerRecord,
) -> Result<(), InfrastructureError> {
    transaction
        .execute(
            "INSERT INTO android_runtime_owners(
                serial, epoch, mode, profile_id, state, source, transition_reason,
                reverse_ports_json, resume_state, runtime_endpoints_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.owner.serial,
                record.owner.epoch.to_string(),
                mode_text(record.owner.mode),
                record.owner.profile_id,
                state_text(record.owner.state),
                source_text(record.owner.source),
                reason_text(record.owner.transition_reason),
                encode_json(&record.reverse_ports)?,
                record.resume_state.map(state_text),
                encode_json(&record.runtime_endpoints)?,
                record.owner.updated_at.to_rfc3339(),
            ],
        )
        .map(|_| ())
        .map_err(database_error)
}

fn encode_json(value: &impl Serialize) -> Result<String, InfrastructureError> {
    serde_json::to_string(value).map_err(|error| corrupt(error.to_string()))
}

type OwnerRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    String,
);

fn parse_record(row: OwnerRow) -> Result<AndroidRuntimeOwnerRecord, InfrastructureError> {
    let (
        serial,
        epoch,
        mode,
        profile_id,
        state,
        source,
        reason,
        ports,
        resume_state,
        endpoints,
        updated_at,
    ) = row;
    Ok(AndroidRuntimeOwnerRecord {
        owner: AndroidRuntimeOwnerViewModel {
            serial,
            epoch: Uuid::parse_str(&epoch)
                .map_err(|error| corrupt(format!("epoch 无效：{error}")))?,
            mode: parse_mode(&mode)?,
            profile_id,
            state: parse_state(&state)?,
            source: parse_source(&source)?,
            transition_reason: parse_reason(&reason)?,
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map_err(|error| corrupt(format!("updated_at 无效：{error}")))?
                .with_timezone(&Utc),
        },
        reverse_ports: serde_json::from_str(&ports)
            .map_err(|error| corrupt(format!("reverse_ports_json 无效：{error}")))?,
        resume_state: resume_state.as_deref().map(parse_state).transpose()?,
        runtime_endpoints: serde_json::from_str(&endpoints)
            .map_err(|error| corrupt(format!("runtime_endpoints_json 无效：{error}")))?,
    })
}

const fn mode_text(value: AndroidRuntimeOwnerMode) -> &'static str {
    match value {
        AndroidRuntimeOwnerMode::DeviceOnly => "device_only",
        AndroidRuntimeOwnerMode::Lan => "lan",
        AndroidRuntimeOwnerMode::AdbReverse => "adb_reverse",
    }
}

const fn state_text(value: AndroidRuntimeOwnerState) -> &'static str {
    match value {
        AndroidRuntimeOwnerState::Active => "active",
        AndroidRuntimeOwnerState::Uncertain => "uncertain",
        AndroidRuntimeOwnerState::WaitingReconnect => "waiting_reconnect",
        AndroidRuntimeOwnerState::CleanupRequired => "cleanup_required",
        AndroidRuntimeOwnerState::StopFailed => "stop_failed",
        AndroidRuntimeOwnerState::Faulted => "faulted",
    }
}

const fn source_text(value: AndroidRuntimeOwnerSource) -> &'static str {
    match value {
        AndroidRuntimeOwnerSource::Start => "start",
        AndroidRuntimeOwnerSource::Apply => "apply",
        AndroidRuntimeOwnerSource::Recovery => "recovery",
    }
}

const fn reason_text(value: AndroidRuntimeOwnerTransitionReason) -> &'static str {
    match value {
        AndroidRuntimeOwnerTransitionReason::ActivationConfirmed => "activation_confirmed",
        AndroidRuntimeOwnerTransitionReason::ActivationUncertain => "activation_uncertain",
        AndroidRuntimeOwnerTransitionReason::ReversePreparation => "reverse_preparation",
        AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired => "reverse_cleanup_required",
        AndroidRuntimeOwnerTransitionReason::DeviceDisconnected => "device_disconnected",
        AndroidRuntimeOwnerTransitionReason::DeviceReconnected => "device_reconnected",
        AndroidRuntimeOwnerTransitionReason::StopFailed => "stop_failed",
        AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage => "recovered_from_storage",
        AndroidRuntimeOwnerTransitionReason::LanEndpointReapplied => "lan_endpoint_reapplied",
        AndroidRuntimeOwnerTransitionReason::LanEndpointFaulted => "lan_endpoint_faulted",
    }
}

fn parse_mode(value: &str) -> Result<AndroidRuntimeOwnerMode, InfrastructureError> {
    match value {
        "device_only" => Ok(AndroidRuntimeOwnerMode::DeviceOnly),
        "lan" => Ok(AndroidRuntimeOwnerMode::Lan),
        "adb_reverse" => Ok(AndroidRuntimeOwnerMode::AdbReverse),
        _ => Err(corrupt(format!("mode 无效：{value}"))),
    }
}

fn parse_state(value: &str) -> Result<AndroidRuntimeOwnerState, InfrastructureError> {
    match value {
        "active" => Ok(AndroidRuntimeOwnerState::Active),
        "uncertain" => Ok(AndroidRuntimeOwnerState::Uncertain),
        "waiting_reconnect" => Ok(AndroidRuntimeOwnerState::WaitingReconnect),
        "cleanup_required" => Ok(AndroidRuntimeOwnerState::CleanupRequired),
        "stop_failed" => Ok(AndroidRuntimeOwnerState::StopFailed),
        "faulted" => Ok(AndroidRuntimeOwnerState::Faulted),
        _ => Err(corrupt(format!("state 无效：{value}"))),
    }
}

fn parse_source(value: &str) -> Result<AndroidRuntimeOwnerSource, InfrastructureError> {
    match value {
        "start" => Ok(AndroidRuntimeOwnerSource::Start),
        "apply" => Ok(AndroidRuntimeOwnerSource::Apply),
        "recovery" => Ok(AndroidRuntimeOwnerSource::Recovery),
        _ => Err(corrupt(format!("source 无效：{value}"))),
    }
}

fn parse_reason(value: &str) -> Result<AndroidRuntimeOwnerTransitionReason, InfrastructureError> {
    match value {
        "activation_confirmed" => Ok(AndroidRuntimeOwnerTransitionReason::ActivationConfirmed),
        "activation_uncertain" => Ok(AndroidRuntimeOwnerTransitionReason::ActivationUncertain),
        "reverse_preparation" => Ok(AndroidRuntimeOwnerTransitionReason::ReversePreparation),
        "reverse_cleanup_required" => {
            Ok(AndroidRuntimeOwnerTransitionReason::ReverseCleanupRequired)
        }
        "device_disconnected" => Ok(AndroidRuntimeOwnerTransitionReason::DeviceDisconnected),
        "device_reconnected" => Ok(AndroidRuntimeOwnerTransitionReason::DeviceReconnected),
        "stop_failed" => Ok(AndroidRuntimeOwnerTransitionReason::StopFailed),
        "recovered_from_storage" => Ok(AndroidRuntimeOwnerTransitionReason::RecoveredFromStorage),
        "lan_endpoint_reapplied" => Ok(AndroidRuntimeOwnerTransitionReason::LanEndpointReapplied),
        "lan_endpoint_faulted" => Ok(AndroidRuntimeOwnerTransitionReason::LanEndpointFaulted),
        _ => Err(corrupt(format!("transition_reason 无效：{value}"))),
    }
}

fn database_error(source: rusqlite::Error) -> InfrastructureError {
    InfrastructureError::Database { source }
}

fn capacity_error() -> InfrastructureError {
    database_error(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER),
        Some("ANDROID_RUNTIME_CAPACITY_EXCEEDED".into()),
    ))
}

fn corrupt(message: String) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "android_runtime_owners",
        message,
    }
}
