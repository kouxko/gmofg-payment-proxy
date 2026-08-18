//! `LocalResponder` request 的 Domain Document 到协议中立有界预览映射。

use std::fmt::Write as _;

use intercept_proxy_domain::{Document, DocumentValue};
use intercept_proxy_protocol_scripting::LocalRequestOutput;
use intercept_proxy_runtime::{
    LocalResponderDiagnostics, SocketDocumentFieldPreview, SocketDocumentPreview,
    SocketLocalRequestPreview,
};
use uuid::Uuid;

const DOCUMENT_PREVIEW_MAX_BYTES: usize = 16 * 1024;

pub(super) fn request_preview(
    exchange_id: Uuid,
    request: &LocalRequestOutput,
) -> SocketLocalRequestPreview {
    SocketLocalRequestPreview::new(
        exchange_id,
        request.origin(),
        Some(document_preview(request.document())),
    )
}

pub(super) fn publish_request_parsed(
    diagnostics: Option<&LocalResponderDiagnostics>,
    exchange_id: intercept_proxy_application::SocketExchangeId,
    request: &LocalRequestOutput,
) {
    let Some(diagnostics) = diagnostics else {
        return;
    };
    let preview = request_preview(exchange_id.as_uuid(), request);
    // Observer 是可丢旁路；宿主 panic 不能改变 response 或关闭连接。
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        diagnostics.request_parsed(preview);
    }));
}

fn document_preview(document: &Document) -> SocketDocumentPreview {
    let schema_id = document.schema().id().as_str().to_owned();
    let schema_version = document.schema().version().to_string();
    let mut remaining = DOCUMENT_PREVIEW_MAX_BYTES
        .saturating_sub(schema_id.len())
        .saturating_sub(schema_version.len());
    let mut fields = Vec::new();
    for state in document.fields() {
        let name = state.field.name().as_str().to_owned();
        let label = state.field.label().to_owned();
        let field_type = state.field.field_type().as_str().to_owned();
        let base = name
            .len()
            .saturating_add(label.len())
            .saturating_add(field_type.len())
            .saturating_add(3);
        if base > remaining {
            // 把第一个超预算字段交给 Proxy 的规范化构造器，它会停止并标记整个
            // Document preview 为 truncated；不再复制其值或后续字段。
            fields.push(SocketDocumentFieldPreview {
                name,
                label,
                field_type,
                present: state.value.is_some(),
                value: None,
                value_truncated: false,
                value_omitted: state.value.is_some(),
            });
            break;
        }
        remaining -= base;
        let (value, value_truncated, value_omitted) =
            state.value.map_or((None, false, false), |value| {
                preview_value(value, remaining)
            });
        remaining = remaining.saturating_sub(value.as_ref().map_or(0, String::len));
        fields.push(SocketDocumentFieldPreview {
            name,
            label,
            field_type,
            present: state.value.is_some(),
            value,
            value_truncated,
            value_omitted,
        });
    }
    SocketDocumentPreview::new(schema_id, schema_version, fields)
}

fn preview_value(value: &DocumentValue, maximum_bytes: usize) -> (Option<String>, bool, bool) {
    match value {
        DocumentValue::String(value) => {
            let preview = truncate_utf8(value, maximum_bytes);
            let truncated = preview.len() < value.len();
            if preview.is_empty() && !value.is_empty() {
                (None, false, true)
            } else {
                (Some(preview), truncated, false)
            }
        }
        DocumentValue::Int(value) => bounded_scalar(value.to_string(), maximum_bytes),
        DocumentValue::Bool(value) => bounded_scalar(value.to_string(), maximum_bytes),
        DocumentValue::Blob(value) => {
            let retained = value.len().min(maximum_bytes / 2);
            let mut preview = String::with_capacity(retained * 2);
            for byte in &value[..retained] {
                let _ = write!(&mut preview, "{byte:02x}");
            }
            if preview.is_empty() && !value.is_empty() {
                (None, false, true)
            } else {
                (Some(preview), retained < value.len(), false)
            }
        }
    }
}

fn bounded_scalar(value: String, maximum_bytes: usize) -> (Option<String>, bool, bool) {
    if value.len() <= maximum_bytes {
        (Some(value), false, false)
    } else {
        (None, false, true)
    }
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

#[cfg(test)]
#[path = "preview/tests.rs"]
mod tests;
