//! `LocalResponder` request 的有界实时预览与观察句柄。

use std::{fmt, sync::Arc, time::SystemTime};

use uuid::Uuid;

use super::{SocketConnectionEvent, SocketConnectionObserver, SocketRelayRunContext};

/// 单个 request 保留的原始字节预览上限。
pub const LOCAL_REQUEST_ORIGIN_PREVIEW_MAX_BYTES: usize = 4 * 1024;
/// 单个 request 保留的 Document 预览逻辑字节上限。
pub const LOCAL_REQUEST_DOCUMENT_PREVIEW_MAX_BYTES: usize = 16 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub struct SocketDocumentFieldPreview {
    pub name: String,
    pub label: String,
    pub field_type: String,
    pub present: bool,
    pub value: Option<String>,
    pub value_truncated: bool,
    pub value_omitted: bool,
}

impl fmt::Debug for SocketDocumentFieldPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentFieldPreview")
            .field("name", &self.name)
            .field("field_type", &self.field_type)
            .field("present", &self.present)
            .field("has_value", &self.value.is_some())
            .field("value_truncated", &self.value_truncated)
            .field("value_omitted", &self.value_omitted)
            .finish_non_exhaustive()
    }
}

impl SocketDocumentFieldPreview {
    fn base_logical_bytes(&self) -> usize {
        self.name
            .len()
            .saturating_add(self.label.len())
            .saturating_add(self.field_type.len())
            .saturating_add(3)
    }

    fn logical_bytes(&self) -> usize {
        self.base_logical_bytes()
            .saturating_add(self.value.as_ref().map_or(0, String::len))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SocketDocumentPreview {
    pub schema_id: String,
    pub schema_version: String,
    pub fields: Vec<SocketDocumentFieldPreview>,
    pub truncated: bool,
}

impl fmt::Debug for SocketDocumentPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketDocumentPreview")
            .field("schema_id", &self.schema_id)
            .field("schema_version", &self.schema_version)
            .field("field_count", &self.fields.len())
            .field("truncated", &self.truncated)
            .finish_non_exhaustive()
    }
}

impl SocketDocumentPreview {
    /// 按 Schema 字段顺序创建预览，并在 16 KiB 总预算内截断值或省略尾部字段。
    #[must_use]
    pub fn new(
        schema_id: String,
        schema_version: String,
        fields: Vec<SocketDocumentFieldPreview>,
    ) -> Self {
        let original_field_count = fields.len();
        let mut remaining = LOCAL_REQUEST_DOCUMENT_PREVIEW_MAX_BYTES
            .saturating_sub(schema_id.len())
            .saturating_sub(schema_version.len());
        let mut retained = Vec::with_capacity(fields.len());
        let mut truncated = false;
        for mut field in fields {
            let base = field.base_logical_bytes();
            if base > remaining {
                truncated = true;
                break;
            }
            remaining -= base;
            if let Some(value) = field.value.take() {
                let original_len = value.len();
                let preview = truncate_utf8(&value, remaining);
                remaining = remaining.saturating_sub(preview.len());
                if preview.is_empty() && !value.is_empty() {
                    field.value_omitted = true;
                    field.value_truncated = false;
                    truncated = true;
                } else {
                    field.value_truncated |= preview.len() < original_len;
                    truncated |= field.value_truncated;
                    field.value = Some(preview);
                }
            }
            truncated |= field.value_omitted;
            retained.push(field);
        }
        truncated |= retained.len() < original_field_count;
        Self {
            schema_id,
            schema_version,
            fields: retained,
            truncated,
        }
    }

    fn logical_bytes(&self) -> usize {
        self.schema_id
            .len()
            .saturating_add(self.schema_version.len())
            .saturating_add(
                self.fields
                    .iter()
                    .map(SocketDocumentFieldPreview::logical_bytes)
                    .sum::<usize>(),
            )
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SocketLocalRequestPreview {
    pub exchange_id: Uuid,
    pub origin_len: u64,
    pub origin_preview: Vec<u8>,
    pub origin_truncated: bool,
    pub document: Option<SocketDocumentPreview>,
}

impl fmt::Debug for SocketLocalRequestPreview {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketLocalRequestPreview")
            .field("exchange_id", &self.exchange_id)
            .field("origin_len", &self.origin_len)
            .field("origin_preview_len", &self.origin_preview.len())
            .field("origin_truncated", &self.origin_truncated)
            .field("has_document", &self.document.is_some())
            .finish_non_exhaustive()
    }
}

impl SocketLocalRequestPreview {
    #[must_use]
    pub fn new(exchange_id: Uuid, origin: &[u8], document: Option<SocketDocumentPreview>) -> Self {
        let preview_len = origin.len().min(LOCAL_REQUEST_ORIGIN_PREVIEW_MAX_BYTES);
        Self {
            exchange_id,
            origin_len: u64::try_from(origin.len()).unwrap_or(u64::MAX),
            origin_preview: origin[..preview_len].to_vec(),
            origin_truncated: preview_len < origin.len(),
            document,
        }
    }

    pub(super) fn logical_bytes(&self) -> usize {
        16_usize
            .saturating_add(self.origin_preview.len())
            .saturating_add(
                self.document
                    .as_ref()
                    .map_or(0, SocketDocumentPreview::logical_bytes),
            )
    }
}

/// Proxy 在连接接纳后注入 processor 的有界旁路句柄。
#[derive(Clone)]
pub struct LocalResponderDiagnostics {
    run: SocketRelayRunContext,
    connection_id: Uuid,
    observer: Arc<dyn SocketConnectionObserver>,
}

impl LocalResponderDiagnostics {
    pub(crate) fn new(
        run: SocketRelayRunContext,
        connection_id: Uuid,
        observer: Arc<dyn SocketConnectionObserver>,
    ) -> Self {
        Self {
            run,
            connection_id,
            observer,
        }
    }

    /// 发布 request 已解析事件。该调用同步、有界且不得执行网络或磁盘 I/O。
    pub fn request_parsed(&self, preview: SocketLocalRequestPreview) {
        self.observer.record(SocketConnectionEvent::RequestParsed {
            run: self.run.clone(),
            connection_id: self.connection_id,
            preview,
            at: SystemTime::now(),
        });
    }
}

impl fmt::Debug for LocalResponderDiagnostics {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalResponderDiagnostics")
            .field("listener_id", &self.run.listener_id)
            .field("connection_id", &self.connection_id)
            .finish_non_exhaustive()
    }
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(test)]
mod tests;
