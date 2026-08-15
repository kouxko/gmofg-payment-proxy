//! Socket 抓包的持久化表与有界查询。
//!
//! HTTP 抓包仍使用原有内存会话模型；本模块只保存已经完成的 Socket Relay Frame 或
//! LocalExchange。运行时诊断（例如 `RequestParsed`）不得写入本表，避免把尚未写回的
//! 半成品误报成正式抓包。

use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::sqlite::socket_capture_coordination::{
    SocketCaptureCompletionPermit, SocketCaptureGeneration,
};
use crate::{InfrastructureError, SqliteStore};

mod query;
mod rows;
use rows::{parse_row, read_row};

pub const DEFAULT_SOCKET_CAPTURE_MAX_RECORDS: u64 = 4_096;
pub const DEFAULT_SOCKET_CAPTURE_MAX_LOGICAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoredSocketCaptureKind {
    RelayFrame,
    LocalExchange,
}

impl StoredSocketCaptureKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::RelayFrame => "relay_frame",
            Self::LocalExchange => "local_exchange",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, SocketCaptureStoreError> {
        match value {
            "relay_frame" => Ok(Self::RelayFrame),
            "local_exchange" => Ok(Self::LocalExchange),
            _ => Err(corrupt("kind 不在允许集合内")),
        }
    }
}

/// Application 已完成校验的正式抓包及其可索引元数据。
#[derive(Clone, PartialEq)]
pub struct SocketCaptureInsert {
    pub capture_id: Uuid,
    pub runtime_epoch: Uuid,
    pub workspace_id: Uuid,
    pub listener_id: Uuid,
    pub session_id: Uuid,
    pub connection_id: Uuid,
    pub occurred_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub kind: StoredSocketCaptureKind,
    /// Relay 必须有方向，LocalExchange 必须为 `None`。
    pub direction: Option<String>,
    pub package_id: String,
    pub package_version: String,
    /// 由 Application 聚合根计算；不依赖 JSON 编码细节或 `SQLite` 页大小。
    pub logical_bytes: u64,
    /// 完整 `SocketCaptureRecord` 的严格 JSON object。
    pub payload: Value,
}

impl std::fmt::Debug for SocketCaptureInsert {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SocketCaptureInsert")
            .field("capture_id", &self.capture_id)
            .field("runtime_epoch", &self.runtime_epoch)
            .field("workspace_id", &self.workspace_id)
            .field("listener_id", &self.listener_id)
            .field("session_id", &self.session_id)
            .field("connection_id", &self.connection_id)
            .field("occurred_at", &self.occurred_at)
            .field("completed_at", &self.completed_at)
            .field("kind", &self.kind)
            .field("direction", &self.direction)
            .field("package_id", &self.package_id)
            .field("package_version", &self.package_version)
            .field("logical_bytes", &self.logical_bytes)
            .field(
                "payload_object_fields",
                &self.payload.as_object().map_or(0, serde_json::Map::len),
            )
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct StoredSocketCapture {
    pub sequence: u64,
    pub capture: SocketCaptureInsert,
}

impl std::fmt::Debug for StoredSocketCapture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredSocketCapture")
            .field("sequence", &self.sequence)
            .field("capture", &self.capture)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketCaptureStorageSort {
    OccurredAt,
    CompletedAt,
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketCaptureStorageSortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketCaptureStorageQuery {
    pub workspace_id: Option<Uuid>,
    pub listener_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub connection_id: Option<Uuid>,
    pub package: Option<(String, String)>,
    pub kind: Option<StoredSocketCaptureKind>,
    pub direction: Option<String>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub sort: SocketCaptureStorageSort,
    pub sort_direction: SocketCaptureStorageSortDirection,
    pub page: u32,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SocketCaptureStoragePage {
    pub rows: Vec<StoredSocketCapture>,
    pub total: u64,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
}

#[derive(Debug, Error)]
pub enum SocketCaptureStoreError {
    #[error(transparent)]
    Infrastructure(#[from] InfrastructureError),
    #[error("Socket 抓包输入无效：{message}")]
    InvalidRecord { message: &'static str },
    #[error("Socket 抓包超过单条持久化上限（{actual_bytes}/{max_bytes} 字节）")]
    PayloadTooLarge { actual_bytes: u64, max_bytes: u64 },
}

#[derive(Debug, Clone, Copy)]
pub struct SocketCaptureRetention {
    pub max_records: u64,
    pub max_logical_bytes: u64,
}

impl Default for SocketCaptureRetention {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_SOCKET_CAPTURE_MAX_RECORDS,
            max_logical_bytes: DEFAULT_SOCKET_CAPTURE_MAX_LOGICAL_BYTES,
        }
    }
}

pub(super) fn create_schema(transaction: &Transaction<'_>) -> Result<(), InfrastructureError> {
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS socket_captures (
                sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                capture_id TEXT NOT NULL UNIQUE,
                runtime_epoch TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                listener_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                connection_id TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                completed_at TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('relay_frame', 'local_exchange')),
                direction TEXT NULL,
                package_id TEXT NOT NULL,
                package_version TEXT NOT NULL,
                logical_bytes INTEGER NOT NULL CHECK(logical_bytes > 0),
                payload_json TEXT NOT NULL
                    CHECK(json_valid(payload_json) AND json_type(payload_json) = 'object'),
                CHECK(
                    (kind = 'relay_frame' AND direction IS NOT NULL)
                    OR (kind = 'local_exchange' AND direction IS NULL)
                )
            );
            CREATE INDEX IF NOT EXISTS socket_captures_workspace_sequence
                ON socket_captures(workspace_id, sequence DESC);
            CREATE INDEX IF NOT EXISTS socket_captures_listener_sequence
                ON socket_captures(listener_id, sequence DESC);
            CREATE INDEX IF NOT EXISTS socket_captures_session_sequence
                ON socket_captures(session_id, sequence DESC);
            CREATE INDEX IF NOT EXISTS socket_captures_connection_sequence
                ON socket_captures(connection_id, sequence DESC);",
        )
        .map_err(|source| InfrastructureError::DatabaseMigration { source })
}

impl SqliteStore {
    /// 发布端只短暂读取 Workspace counter Map，之后仅执行原子读取，不争用持久化 gate。
    pub(crate) fn socket_capture_generation(&self, workspace_id: Uuid) -> SocketCaptureGeneration {
        self.capture_coordination.snapshot(workspace_id)
    }

    pub(crate) fn socket_capture_completion_if_current(
        &self,
        generation: &SocketCaptureGeneration,
    ) -> Option<SocketCaptureCompletionPermit<'_>> {
        self.capture_coordination.completion_if_current(generation)
    }

    pub fn insert_socket_capture(
        &self,
        capture: &SocketCaptureInsert,
    ) -> Result<StoredSocketCapture, SocketCaptureStoreError> {
        self.insert_socket_capture_with_retention(capture, SocketCaptureRetention::default())
    }

    pub fn insert_socket_capture_with_retention(
        &self,
        capture: &SocketCaptureInsert,
        retention: SocketCaptureRetention,
    ) -> Result<StoredSocketCapture, SocketCaptureStoreError> {
        let _gate = self.capture_coordination.mutation_gate.lock();
        self.insert_socket_capture_locked(capture, retention)
    }

    /// 仅当记录入队后没有跨过 clear/reset 线性化点时写入。
    pub(crate) fn insert_socket_capture_if_current(
        &self,
        capture: &SocketCaptureInsert,
        generation: &SocketCaptureGeneration,
    ) -> Result<Option<StoredSocketCapture>, SocketCaptureStoreError> {
        let _gate = self.capture_coordination.mutation_gate.lock();
        if !self.capture_coordination.is_current(generation) {
            return Ok(None);
        }
        self.insert_socket_capture_locked(capture, SocketCaptureRetention::default())
            .map(Some)
    }

    fn insert_socket_capture_locked(
        &self,
        capture: &SocketCaptureInsert,
        retention: SocketCaptureRetention,
    ) -> Result<StoredSocketCapture, SocketCaptureStoreError> {
        validate_capture(capture, retention)?;
        let payload_json =
            serde_json::to_string(&capture.payload).map_err(|_| invalid("payload 无法序列化"))?;
        let logical_bytes = i64::try_from(capture.logical_bytes)
            .map_err(|_| invalid("logical_bytes 超出 SQLite 整数范围"))?;
        let mut connection = self.connection.lock();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO socket_captures(
                    capture_id, runtime_epoch, workspace_id, listener_id, session_id,
                    connection_id, occurred_at, completed_at, kind, direction,
                    package_id, package_version, logical_bytes, payload_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    capture.capture_id.to_string(),
                    capture.runtime_epoch.to_string(),
                    capture.workspace_id.to_string(),
                    capture.listener_id.to_string(),
                    capture.session_id.to_string(),
                    capture.connection_id.to_string(),
                    capture.occurred_at.to_rfc3339(),
                    capture.completed_at.to_rfc3339(),
                    capture.kind.as_str(),
                    capture.direction,
                    capture.package_id,
                    capture.package_version,
                    logical_bytes,
                    payload_json,
                ],
            )
            .map_err(database_error)?;
        let sequence =
            u64::try_from(transaction.last_insert_rowid()).map_err(|_| corrupt("sequence 无效"))?;
        enforce_retention(&transaction, retention)?;
        transaction.commit().map_err(database_error)?;
        Ok(StoredSocketCapture {
            sequence,
            capture: capture.clone(),
        })
    }

    pub fn get_socket_capture(
        &self,
        capture_id: Uuid,
    ) -> Result<Option<StoredSocketCapture>, SocketCaptureStoreError> {
        let connection = self.connection.lock();
        connection
            .query_row(
                &format!(
                    "{} WHERE capture_id = ?1",
                    rows::SELECT_SOCKET_CAPTURE_COLUMNS
                ),
                [capture_id.to_string()],
                read_row,
            )
            .optional()
            .map_err(database_error)?
            .map(parse_row)
            .transpose()
    }

    /// `None` 仅供完整应用重置使用；普通 UI 清理必须带 Workspace id。
    pub fn clear_socket_captures(
        &self,
        workspace_id: Option<Uuid>,
    ) -> Result<u64, SocketCaptureStoreError> {
        let _completion_gate = self.capture_coordination.completion_gate.write();
        let _gate = self.capture_coordination.mutation_gate.lock();
        if let Some(workspace_id) = workspace_id {
            self.capture_coordination
                .bump_workspace(workspace_id)
                .map_err(corrupt)?;
        } else {
            self.capture_coordination.bump_reset().map_err(corrupt)?;
        }
        let connection = self.connection.lock();
        let deleted = if let Some(workspace_id) = workspace_id {
            connection.execute(
                "DELETE FROM socket_captures WHERE workspace_id = ?1",
                [workspace_id.to_string()],
            )
        } else {
            connection.execute("DELETE FROM socket_captures", [])
        }
        .map_err(database_error)?;
        let deleted = u64::try_from(deleted).map_err(|_| corrupt("删除条数无效"))?;
        Ok(deleted)
    }

    #[cfg(test)]
    pub(crate) fn block_socket_capture_mutation_for_test(
        &self,
        entered: &std::sync::mpsc::Sender<()>,
        release: &std::sync::mpsc::Receiver<()>,
    ) {
        let _gate = self.capture_coordination.mutation_gate.lock();
        let _ = entered.send(());
        let _ = release.recv();
    }
}

fn validate_capture(
    capture: &SocketCaptureInsert,
    retention: SocketCaptureRetention,
) -> Result<(), SocketCaptureStoreError> {
    if retention.max_records == 0 || retention.max_logical_bytes == 0 {
        return Err(invalid("保留上限必须大于零"));
    }
    if capture.logical_bytes == 0 {
        return Err(invalid("logical_bytes 必须大于零"));
    }
    if capture.logical_bytes > retention.max_logical_bytes {
        return Err(SocketCaptureStoreError::PayloadTooLarge {
            actual_bytes: capture.logical_bytes,
            max_bytes: retention.max_logical_bytes,
        });
    }
    if !capture.payload.is_object() {
        return Err(invalid("payload 必须是 JSON object"));
    }
    if capture.completed_at < capture.occurred_at {
        return Err(invalid("completed_at 不得早于 occurred_at"));
    }
    if capture.package_id.trim().is_empty() || capture.package_version.trim().is_empty() {
        return Err(invalid("精确协议包身份不能为空"));
    }
    let direction_valid = capture
        .direction
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && value.len() <= 128);
    match capture.kind {
        StoredSocketCaptureKind::RelayFrame if !direction_valid => {
            Err(invalid("RelayFrame 必须提供合法 direction"))
        }
        StoredSocketCaptureKind::LocalExchange if capture.direction.is_some() => {
            Err(invalid("LocalExchange 不得伪造 frame direction"))
        }
        _ => Ok(()),
    }
}

fn enforce_retention(
    transaction: &Transaction<'_>,
    retention: SocketCaptureRetention,
) -> Result<(), SocketCaptureStoreError> {
    loop {
        let (count, bytes): (i64, i64) = transaction
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(logical_bytes), 0) FROM socket_captures",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(database_error)?;
        if non_negative_u64(count, "count")? <= retention.max_records
            && non_negative_u64(bytes, "logical_bytes sum")? <= retention.max_logical_bytes
        {
            return Ok(());
        }
        let deleted = transaction
            .execute(
                "DELETE FROM socket_captures WHERE sequence =
                    (SELECT sequence FROM socket_captures ORDER BY sequence ASC LIMIT 1)",
                [],
            )
            .map_err(database_error)?;
        if deleted != 1 {
            return Err(corrupt("保留策略无法删除最旧记录"));
        }
    }
}

pub(super) fn database_error(source: rusqlite::Error) -> SocketCaptureStoreError {
    InfrastructureError::Database { source }.into()
}

pub(super) fn corrupt(message: impl Into<String>) -> SocketCaptureStoreError {
    InfrastructureError::PersistenceCorrupt {
        entity: "socket_capture",
        message: message.into(),
    }
    .into()
}

pub(super) fn non_negative_u64(
    value: i64,
    field: &'static str,
) -> Result<u64, SocketCaptureStoreError> {
    u64::try_from(value).map_err(|_| corrupt(field))
}

const fn invalid(message: &'static str) -> SocketCaptureStoreError {
    SocketCaptureStoreError::InvalidRecord { message }
}
