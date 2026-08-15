//! Application Socket capture DTO 与 `SQLite` 严格记录之间的适配器。
//!
//! 本模块不接触旧 HTTP capture/session。写入只接受完整 `SocketCaptureRecord`，读取时
//! 同时反序列化严格 JSON 并核对全部索引列，防止“索引显示 A、详情实际是 B”的损坏
//! 数据被静默展示。

use std::sync::Arc;

use intercept_proxy_application::{
    AppError, AppResult, SocketCaptureDetailViewModel, SocketCaptureKind,
    SocketCapturePageViewModel, SocketCapturePayload, SocketCaptureQuery, SocketCaptureRecord,
    SocketCaptureRowViewModel, SocketCaptureSort, SortDirection,
};
use intercept_proxy_domain::SocketDirection;

use crate::{InfrastructureError, SqliteStore};

use super::common::app_error;
use crate::sqlite::socket_capture_coordination::SocketCaptureCompletionPermit;
use crate::sqlite::socket_capture_coordination::SocketCaptureGeneration;
use crate::sqlite::socket_captures::{
    SocketCaptureInsert, SocketCaptureStorageQuery, SocketCaptureStorageSort,
    SocketCaptureStorageSortDirection, SocketCaptureStoreError, StoredSocketCapture,
    StoredSocketCaptureKind,
};

#[derive(Debug)]
pub struct SocketCaptureRepositoryAdapter {
    store: Arc<SqliteStore>,
}

impl SocketCaptureRepositoryAdapter {
    #[must_use]
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }

    /// 运行时只应在完整 Frame/Exchange 已成功写出后调用该方法。
    /// 独立持久化 worker 按值移交所有权，调用返回后不会继续持有大型 Frame。
    #[allow(clippy::needless_pass_by_value)]
    pub fn record(&self, record: SocketCaptureRecord) -> AppResult<SocketCaptureRowViewModel> {
        let row = row_from_record(&record);
        let insert = insert_from_record(&record)?;
        self.store
            .insert_socket_capture(&insert)
            .map_err(map_store_error)?;
        Ok(row)
    }

    pub(crate) fn generation_for(
        &self,
        workspace_id: intercept_proxy_application::WorkspaceId,
    ) -> SocketCaptureGeneration {
        self.store.socket_capture_generation(workspace_id.as_uuid())
    }

    /// drain worker 使用入队代次提交记录；`None` 表示 clear/reset 已越过该记录，必须丢弃。
    pub(crate) fn record_if_current(
        &self,
        record: &SocketCaptureRecord,
        generation: &SocketCaptureGeneration,
    ) -> AppResult<Option<SocketCaptureRowViewModel>> {
        let row = row_from_record(record);
        let insert = insert_from_record(record)?;
        self.store
            .insert_socket_capture_if_current(&insert, generation)
            .map_err(map_store_error)
            .map(|stored| stored.map(|_| row))
    }

    pub(crate) fn completion_if_current(
        &self,
        generation: &SocketCaptureGeneration,
    ) -> Option<SocketCaptureCompletionPermit<'_>> {
        self.store.socket_capture_completion_if_current(generation)
    }

    pub fn query(&self, query: &SocketCaptureQuery) -> AppResult<SocketCapturePageViewModel> {
        let stored = self
            .store
            .query_socket_captures(&storage_query(query))
            .map_err(map_store_error)?;
        let rows = stored
            .rows
            .into_iter()
            .map(|stored| decode_stored(&stored))
            .map(|record| record.map(|record| row_from_record(&record)))
            .collect::<AppResult<Vec<_>>>()?;
        Ok(SocketCapturePageViewModel {
            rows,
            total: usize::try_from(stored.total).unwrap_or(usize::MAX),
            page: stored.page,
            page_size: stored.page_size,
            total_pages: stored.total_pages,
            empty_message: if stored.total == 0 {
                "没有符合条件的 Socket 抓包。".to_owned()
            } else {
                String::new()
            },
        })
    }

    pub fn get_detail(
        &self,
        capture_id: intercept_proxy_application::SocketCaptureId,
    ) -> AppResult<SocketCaptureDetailViewModel> {
        let stored = self
            .store
            .get_socket_capture(capture_id.as_uuid())
            .map_err(map_store_error)?
            .ok_or_else(|| {
                AppError::new("SOCKET_CAPTURE_NOT_FOUND", "Socket 抓包不存在或已被淘汰。")
                    .entity(capture_id.to_string())
            })?;
        Ok(SocketCaptureDetailViewModel {
            record: decode_stored(&stored)?,
        })
    }

    pub fn clear_completed(
        &self,
        workspace_id: intercept_proxy_application::WorkspaceId,
    ) -> AppResult<usize> {
        self.store
            .clear_socket_captures(Some(workspace_id.as_uuid()))
            .map_err(map_store_error)
            .map(|count| usize::try_from(count).unwrap_or(usize::MAX))
    }
}

fn insert_from_record(record: &SocketCaptureRecord) -> AppResult<SocketCaptureInsert> {
    if !record.is_consistent() {
        return Err(AppError::new(
            "SOCKET_CAPTURE_INVALID",
            "Socket 抓包内容无效。",
        ));
    }
    let (kind, direction, package) = payload_index(&record.payload);
    let payload = serde_json::to_value(record).map_err(|error| {
        AppError::new(
            "SOCKET_CAPTURE_INVALID",
            format!("Socket 抓包无法序列化：{error}"),
        )
    })?;
    Ok(SocketCaptureInsert {
        capture_id: record.capture_id.as_uuid(),
        runtime_epoch: record.runtime_epoch,
        workspace_id: record.workspace_id.as_uuid(),
        listener_id: record.listener_id.as_uuid(),
        session_id: record.session_id,
        connection_id: record.connection_id.as_uuid(),
        occurred_at: record.occurred_at,
        completed_at: record.completed_at,
        kind,
        direction: direction.map(direction_text).map(str::to_owned),
        package_id: package.id.as_str().to_owned(),
        package_version: package.version.as_str().to_owned(),
        logical_bytes: record.logical_bytes(),
        payload,
    })
}

fn storage_query(query: &SocketCaptureQuery) -> SocketCaptureStorageQuery {
    SocketCaptureStorageQuery {
        workspace_id: query
            .workspace_id
            .map(intercept_proxy_application::WorkspaceId::as_uuid),
        listener_id: query
            .listener_id
            .map(intercept_proxy_application::ListenerId::as_uuid),
        session_id: query.session_id,
        connection_id: query
            .connection_id
            .map(intercept_proxy_application::SocketConnectionId::as_uuid),
        package: query.package.as_ref().map(|package| {
            (
                package.id.as_str().to_owned(),
                package.version.as_str().to_owned(),
            )
        }),
        kind: query.kind.map(storage_kind),
        direction: query.direction.map(direction_text).map(str::to_owned),
        occurred_from: query.occurred_from,
        occurred_to: query.occurred_to,
        sort: match query.sort {
            SocketCaptureSort::OccurredAt => SocketCaptureStorageSort::OccurredAt,
            SocketCaptureSort::CompletedAt => SocketCaptureStorageSort::CompletedAt,
            SocketCaptureSort::Size => SocketCaptureStorageSort::Size,
        },
        sort_direction: match query.direction_sort {
            SortDirection::Asc => SocketCaptureStorageSortDirection::Asc,
            SortDirection::Desc => SocketCaptureStorageSortDirection::Desc,
        },
        page: query.page.page,
        page_size: query.page.page_size,
    }
}

fn decode_stored(stored: &StoredSocketCapture) -> AppResult<SocketCaptureRecord> {
    let record = serde_json::from_value::<SocketCaptureRecord>(stored.capture.payload.clone())
        .map_err(|_| corrupt_app_error())?;
    if !record.is_consistent() {
        return Err(corrupt_app_error());
    }
    let (kind, direction, package) = payload_index(&record.payload);
    let metadata_matches = record.capture_id.as_uuid() == stored.capture.capture_id
        && record.runtime_epoch == stored.capture.runtime_epoch
        && record.workspace_id.as_uuid() == stored.capture.workspace_id
        && record.listener_id.as_uuid() == stored.capture.listener_id
        && record.session_id == stored.capture.session_id
        && record.connection_id.as_uuid() == stored.capture.connection_id
        && record.occurred_at == stored.capture.occurred_at
        && record.completed_at == stored.capture.completed_at
        && kind == stored.capture.kind
        && direction.map(direction_text) == stored.capture.direction.as_deref()
        && package.id.as_str() == stored.capture.package_id
        && package.version.as_str() == stored.capture.package_version
        && record.logical_bytes() == stored.capture.logical_bytes;
    if !metadata_matches {
        return Err(corrupt_app_error());
    }
    Ok(record)
}

fn row_from_record(record: &SocketCaptureRecord) -> SocketCaptureRowViewModel {
    let (kind, direction, package, schema, origin, written, rules) = match &record.payload {
        SocketCapturePayload::RelayFrame(frame) => (
            SocketCaptureKind::RelayFrame,
            Some(frame.direction),
            frame.package.clone(),
            frame.schema.clone(),
            frame.origin.len(),
            frame.written.len(),
            frame.matched_rule_ids.clone(),
        ),
        SocketCapturePayload::LocalExchange(exchange) => (
            SocketCaptureKind::LocalExchange,
            None,
            exchange.package.clone(),
            exchange.schema.clone(),
            exchange.request_origin.len(),
            exchange.written_response.len(),
            exchange.matched_downstream_rule_ids.clone(),
        ),
    };
    SocketCaptureRowViewModel {
        capture_id: record.capture_id,
        runtime_epoch: record.runtime_epoch,
        session_id: record.session_id,
        connection_id: record.connection_id,
        listener_id: record.listener_id,
        occurred_at: record.occurred_at,
        completed_at: record.completed_at,
        kind,
        direction,
        package,
        schema,
        origin_size_bytes: u64::try_from(origin).unwrap_or(u64::MAX),
        written_size_bytes: u64::try_from(written).unwrap_or(u64::MAX),
        logical_size_bytes: record.logical_bytes(),
        matched_rule_ids: rules,
    }
}

fn payload_index(
    payload: &SocketCapturePayload,
) -> (
    StoredSocketCaptureKind,
    Option<SocketDirection>,
    &intercept_proxy_domain::ProtocolPackageRef,
) {
    match payload {
        SocketCapturePayload::RelayFrame(frame) => (
            StoredSocketCaptureKind::RelayFrame,
            Some(frame.direction),
            &frame.package,
        ),
        SocketCapturePayload::LocalExchange(exchange) => (
            StoredSocketCaptureKind::LocalExchange,
            None,
            &exchange.package,
        ),
    }
}

const fn storage_kind(kind: SocketCaptureKind) -> StoredSocketCaptureKind {
    match kind {
        SocketCaptureKind::RelayFrame => StoredSocketCaptureKind::RelayFrame,
        SocketCaptureKind::LocalExchange => StoredSocketCaptureKind::LocalExchange,
    }
}

const fn direction_text(direction: SocketDirection) -> &'static str {
    match direction {
        SocketDirection::Upstream => "upstream",
        SocketDirection::Downstream => "downstream",
    }
}

fn map_store_error(error: SocketCaptureStoreError) -> AppError {
    match error {
        SocketCaptureStoreError::Infrastructure(error) => app_error(error),
        SocketCaptureStoreError::InvalidRecord { .. } => {
            AppError::new("SOCKET_CAPTURE_INVALID", "Socket 抓包内容无效。")
        }
        SocketCaptureStoreError::PayloadTooLarge { .. } => AppError::new(
            "SOCKET_CAPTURE_TOO_LARGE",
            "Socket 抓包超过本地持久化上限。",
        ),
    }
}

fn corrupt_app_error() -> AppError {
    app_error(InfrastructureError::PersistenceCorrupt {
        entity: "socket_capture",
        message: "索引列与严格 JSON 记录不一致".to_owned(),
    })
}

#[cfg(test)]
#[path = "socket_capture/tests.rs"]
mod tests;
