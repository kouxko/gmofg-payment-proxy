//! Socket capture SQL 查询构造器。
//!
//! 所有可变片段都来自封闭枚举；用户值只通过绑定参数进入 SQL。COUNT 与分页 SELECT
//! 使用同一组过滤条件，只有最终页会进入严格 JSON 反序列化边界。

use rusqlite::{params_from_iter, types::Value as SqlValue};

use super::{
    SocketCaptureStoragePage, SocketCaptureStorageQuery, SocketCaptureStorageSort,
    SocketCaptureStorageSortDirection, SocketCaptureStoreError, SqliteStore, database_error,
    parse_row, read_row, rows,
};

impl SqliteStore {
    pub fn query_socket_captures(
        &self,
        query: &SocketCaptureStorageQuery,
    ) -> Result<SocketCaptureStoragePage, SocketCaptureStoreError> {
        let page = query.page.max(1);
        let page_size = query.page_size.clamp(1, 200);
        let (where_sql, parameters) = filters(query);
        let connection = self.connection.lock();
        let total_raw: i64 = connection
            .query_row(
                &format!("SELECT COUNT(*) FROM socket_captures{where_sql}"),
                params_from_iter(parameters.iter()),
                |row| row.get(0),
            )
            .map_err(database_error)?;
        let total = super::non_negative_u64(total_raw, "count")?;
        let offset = u64::from(page.saturating_sub(1)).saturating_mul(u64::from(page_size));
        let mut page_parameters = parameters;
        page_parameters.push(SqlValue::Integer(i64::from(page_size)));
        page_parameters.push(SqlValue::Integer(i64::try_from(offset).unwrap_or(i64::MAX)));
        let mut statement = connection
            .prepare(&format!(
                "{}{where_sql} ORDER BY {} {} , sequence {} LIMIT ? OFFSET ?",
                rows::SELECT_SOCKET_CAPTURE_COLUMNS,
                sort_column(query.sort),
                sort_direction(query.sort_direction),
                sort_direction(query.sort_direction),
            ))
            .map_err(database_error)?;
        let rows = statement
            .query_map(params_from_iter(page_parameters.iter()), read_row)
            .map_err(database_error)?
            .map(|row| row.map_err(database_error).and_then(parse_row))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SocketCaptureStoragePage {
            rows,
            total,
            page,
            page_size,
            total_pages: u32::try_from(total.div_ceil(u64::from(page_size))).unwrap_or(u32::MAX),
        })
    }
}

fn filters(query: &SocketCaptureStorageQuery) -> (String, Vec<SqlValue>) {
    let mut clauses = Vec::new();
    let mut parameters = Vec::new();
    push_uuid(
        &mut clauses,
        &mut parameters,
        "workspace_id",
        query.workspace_id,
    );
    push_uuid(
        &mut clauses,
        &mut parameters,
        "listener_id",
        query.listener_id,
    );
    push_uuid(
        &mut clauses,
        &mut parameters,
        "session_id",
        query.session_id,
    );
    push_uuid(
        &mut clauses,
        &mut parameters,
        "connection_id",
        query.connection_id,
    );
    if let Some((id, version)) = &query.package {
        clauses.push("package_id = ?".to_owned());
        parameters.push(SqlValue::Text(id.clone()));
        clauses.push("package_version = ?".to_owned());
        parameters.push(SqlValue::Text(version.clone()));
    }
    if let Some(kind) = query.kind {
        clauses.push("kind = ?".to_owned());
        parameters.push(SqlValue::Text(kind.as_str().to_owned()));
    }
    if let Some(direction) = &query.direction {
        clauses.push("direction = ?".to_owned());
        parameters.push(SqlValue::Text(direction.clone()));
    }
    if let Some(from) = query.occurred_from {
        clauses.push("occurred_at >= ?".to_owned());
        parameters.push(SqlValue::Text(from.to_rfc3339()));
    }
    if let Some(to) = query.occurred_to {
        clauses.push("occurred_at <= ?".to_owned());
        parameters.push(SqlValue::Text(to.to_rfc3339()));
    }
    let sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    (sql, parameters)
}

fn push_uuid(
    clauses: &mut Vec<String>,
    parameters: &mut Vec<SqlValue>,
    column: &'static str,
    value: Option<uuid::Uuid>,
) {
    if let Some(value) = value {
        clauses.push(format!("{column} = ?"));
        parameters.push(SqlValue::Text(value.to_string()));
    }
}

const fn sort_column(sort: SocketCaptureStorageSort) -> &'static str {
    match sort {
        SocketCaptureStorageSort::OccurredAt => "occurred_at",
        SocketCaptureStorageSort::CompletedAt => "completed_at",
        SocketCaptureStorageSort::Size => "logical_bytes",
    }
}

const fn sort_direction(direction: SocketCaptureStorageSortDirection) -> &'static str {
    match direction {
        SocketCaptureStorageSortDirection::Asc => "ASC",
        SocketCaptureStorageSortDirection::Desc => "DESC",
    }
}
