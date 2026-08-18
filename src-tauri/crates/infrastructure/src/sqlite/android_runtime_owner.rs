use chrono::{DateTime, Utc};
use intercept_proxy_application::{
    AndroidRuntimeEndpointViewModel, AndroidRuntimeOwnerMode, AndroidRuntimeOwnerSource,
    AndroidRuntimeOwnerState, AndroidRuntimeOwnerTransitionReason, AndroidRuntimeOwnerViewModel,
};
use rusqlite::{OptionalExtension, params};
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
    pub fn load_android_runtime_owner(
        &self,
    ) -> Result<Option<AndroidRuntimeOwnerRecord>, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                "SELECT serial, epoch, mode, profile_id, state, source,
                        transition_reason, reverse_ports_json, resume_state, runtime_endpoints_json,
                        updated_at
                 FROM android_runtime_owner WHERE singleton_id = 1",
                [],
                |row| {
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
                },
            )
            .optional()
            .map_err(database_error)?
            .map(parse_record)
            .transpose()
    }

    pub fn save_android_runtime_owner(
        &self,
        record: &AndroidRuntimeOwnerRecord,
    ) -> Result<(), InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "INSERT INTO android_runtime_owner(
                    singleton_id, serial, epoch, mode, profile_id, state, source,
                    transition_reason, reverse_ports_json, resume_state, runtime_endpoints_json,
                    updated_at
                 ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(singleton_id) DO UPDATE SET
                    serial = excluded.serial, epoch = excluded.epoch, mode = excluded.mode,
                    profile_id = excluded.profile_id, state = excluded.state,
                    source = excluded.source, transition_reason = excluded.transition_reason,
                    reverse_ports_json = excluded.reverse_ports_json,
                    resume_state = excluded.resume_state,
                    runtime_endpoints_json = excluded.runtime_endpoints_json,
                    updated_at = excluded.updated_at",
                params![
                    record.owner.serial,
                    record.owner.epoch.to_string(),
                    mode_text(record.owner.mode),
                    record.owner.profile_id,
                    state_text(record.owner.state),
                    source_text(record.owner.source),
                    reason_text(record.owner.transition_reason),
                    serde_json::to_string(&record.reverse_ports)
                        .map_err(|error| corrupt(error.to_string()))?,
                    record.resume_state.map(state_text),
                    serde_json::to_string(&record.runtime_endpoints)
                        .map_err(|error| corrupt(error.to_string()))?,
                    record.owner.updated_at.to_rfc3339(),
                ],
            )
            .map(|_| ())
            .map_err(database_error)
    }

    pub fn clear_android_runtime_owner(
        &self,
        expected_epoch: Uuid,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "DELETE FROM android_runtime_owner WHERE singleton_id = 1 AND epoch = ?1",
                [expected_epoch.to_string()],
            )
            .map(|deleted| deleted == 1)
            .map_err(database_error)
    }

    pub fn replace_android_runtime_owner_if_epoch(
        &self,
        expected_epoch: Uuid,
        record: &AndroidRuntimeOwnerRecord,
    ) -> Result<bool, InfrastructureError> {
        let connection = self.connection.lock();
        connection
            .execute(
                "UPDATE android_runtime_owner SET
                    serial = ?1, mode = ?2, profile_id = ?3, state = ?4, source = ?5,
                    transition_reason = ?6, reverse_ports_json = ?7, resume_state = ?8,
                    runtime_endpoints_json = ?9, updated_at = ?10
                    WHERE singleton_id = 1 AND epoch = ?11",
                params![
                    record.owner.serial,
                    mode_text(record.owner.mode),
                    record.owner.profile_id,
                    state_text(record.owner.state),
                    source_text(record.owner.source),
                    reason_text(record.owner.transition_reason),
                    serde_json::to_string(&record.reverse_ports)
                        .map_err(|error| corrupt(error.to_string()))?,
                    record.resume_state.map(state_text),
                    serde_json::to_string(&record.runtime_endpoints)
                        .map_err(|error| corrupt(error.to_string()))?,
                    record.owner.updated_at.to_rfc3339(),
                    expected_epoch.to_string(),
                ],
            )
            .map(|updated| updated == 1)
            .map_err(database_error)
    }
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
fn corrupt(message: String) -> InfrastructureError {
    InfrastructureError::PersistenceCorrupt {
        entity: "android_runtime_owner",
        message,
    }
}
