//! `SQLite` 行到严格 Socket capture 记录的唯一转换边界。

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use super::{
    SocketCaptureInsert, SocketCaptureStoreError, StoredSocketCapture, StoredSocketCaptureKind,
    corrupt, non_negative_u64,
};

pub(super) const SELECT_SOCKET_CAPTURE_COLUMNS: &str =
    "SELECT sequence, capture_id, runtime_epoch, workspace_id, listener_id, session_id,
            connection_id, occurred_at, completed_at, kind, direction, package_id,
            package_version, logical_bytes, payload_json FROM socket_captures";

pub(super) type RawSocketCaptureRow = (
    i64,
    String,
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
    i64,
    String,
);

pub(super) fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawSocketCaptureRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
    ))
}

pub(super) fn parse_row(
    raw: RawSocketCaptureRow,
) -> Result<StoredSocketCapture, SocketCaptureStoreError> {
    let (
        sequence,
        capture_id,
        runtime_epoch,
        workspace_id,
        listener_id,
        session_id,
        connection_id,
        occurred_at,
        completed_at,
        kind,
        direction,
        package_id,
        package_version,
        logical_bytes,
        payload_json,
    ) = raw;
    let kind = StoredSocketCaptureKind::parse(&kind)?;
    if (kind == StoredSocketCaptureKind::RelayFrame) != direction.is_some() {
        return Err(corrupt("kind 与 direction 不一致"));
    }
    let payload =
        serde_json::from_str::<Value>(&payload_json).map_err(|_| corrupt("payload_json 无效"))?;
    if !payload.is_object() {
        return Err(corrupt("payload_json 必须是 object"));
    }
    let occurred_at = parse_time(&occurred_at, "occurred_at")?;
    let completed_at = parse_time(&completed_at, "completed_at")?;
    if completed_at < occurred_at {
        return Err(corrupt("completed_at 早于 occurred_at"));
    }
    Ok(StoredSocketCapture {
        sequence: positive_u64(sequence, "sequence")?,
        capture: SocketCaptureInsert {
            capture_id: parse_uuid(&capture_id, "capture_id")?,
            runtime_epoch: parse_uuid(&runtime_epoch, "runtime_epoch")?,
            workspace_id: parse_uuid(&workspace_id, "workspace_id")?,
            listener_id: parse_uuid(&listener_id, "listener_id")?,
            session_id: parse_uuid(&session_id, "session_id")?,
            connection_id: parse_uuid(&connection_id, "connection_id")?,
            occurred_at,
            completed_at,
            kind,
            direction,
            package_id,
            package_version,
            logical_bytes: positive_u64(logical_bytes, "logical_bytes")?,
            payload,
        },
    })
}

fn parse_uuid(value: &str, field: &'static str) -> Result<Uuid, SocketCaptureStoreError> {
    Uuid::parse_str(value).map_err(|_| corrupt(field))
}

fn parse_time(value: &str, field: &'static str) -> Result<DateTime<Utc>, SocketCaptureStoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| corrupt(field))
}

fn positive_u64(value: i64, field: &'static str) -> Result<u64, SocketCaptureStoreError> {
    let value = non_negative_u64(value, field)?;
    if value == 0 {
        Err(corrupt(field))
    } else {
        Ok(value)
    }
}
