use intercept_proxy_domain::{
    ConnectionFaultAction, ListenerId, MetadataExtractorSource, ResponseAssertionKind,
};
use uuid::Uuid;

use super::{AppError, AppResult, OperationResultViewModel, ProxyWorkspace, UiTone};

pub(super) fn find_mut<'a, T>(
    items: &'a mut [T],
    component_id: &str,
    id: impl Fn(&T) -> String,
) -> AppResult<&'a mut T> {
    items
        .iter_mut()
        .find(|item| id(item) == component_id)
        .ok_or_else(|| {
            AppError::new(
                "WORKSPACE_COMPONENT_NOT_FOUND",
                "Workspace 组件不存在或已被删除。",
            )
            .entity(component_id.to_owned())
        })
}

pub(super) fn parse_listener_ids(raw: &str) -> AppResult<Vec<ListenerId>> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            Uuid::parse_str(value)
                .map(ListenerId::from_uuid)
                .map_err(|_| {
                    AppError::new(
                        "WORKSPACE_LISTENER_ID_INVALID",
                        format!("代理入口 ID“{value}”不是有效 UUID。"),
                    )
                })
        })
        .collect()
}

pub(super) fn metadata_source(kind: &str) -> AppResult<MetadataExtractorSource> {
    match kind {
        "header" => Ok(MetadataExtractorSource::Header {
            name: String::new(),
        }),
        "json_path" => Ok(MetadataExtractorSource::JsonPath {
            path: "$.field".into(),
        }),
        "body_text" => Ok(MetadataExtractorSource::BodyText),
        "fixed_value" => Ok(MetadataExtractorSource::FixedValue {
            value: String::new(),
        }),
        _ => Err(component_variant_error()),
    }
}

pub(super) fn response_assertion(kind: &str) -> AppResult<ResponseAssertionKind> {
    match kind {
        "http_status_equals" => Ok(ResponseAssertionKind::HttpStatusEquals { expected: 200 }),
        "header_equals" => Ok(ResponseAssertionKind::HeaderEquals {
            name: String::new(),
            expected: String::new(),
        }),
        "json_path_equals" => Ok(ResponseAssertionKind::JsonPathEquals {
            path: "$.field".into(),
            expected: serde_json::Value::Null,
        }),
        "body_text_contains" => Ok(ResponseAssertionKind::BodyTextContains {
            expected: String::new(),
        }),
        "body_length_equals" => Ok(ResponseAssertionKind::BodyLengthEquals { expected: 0 }),
        "body_sha256_equals" => Ok(ResponseAssertionKind::BodySha256Equals {
            expected_hex: String::new(),
        }),
        _ => Err(component_variant_error()),
    }
}

pub(super) fn connection_fault(kind: &str) -> AppResult<ConnectionFaultAction> {
    match kind {
        "delay" => Ok(ConnectionFaultAction::Delay { milliseconds: 100 }),
        "reject" => Ok(ConnectionFaultAction::Reject),
        "rate_limit" => Ok(ConnectionFaultAction::RateLimit {
            bytes_per_second: 64 * 1024,
        }),
        "close_after_bytes" => Ok(ConnectionFaultAction::CloseAfterBytes { bytes: 1 }),
        "half_close_after_bytes" => Ok(ConnectionFaultAction::HalfCloseAfterBytes { bytes: 1 }),
        "idle_timeout" => Ok(ConnectionFaultAction::IdleTimeout {
            milliseconds: 30_000,
        }),
        _ => Err(component_variant_error()),
    }
}

pub(super) fn component_variant_error() -> AppError {
    AppError::new(
        "WORKSPACE_COMPONENT_VARIANT_INVALID",
        "Workspace 组件类型选项无效。",
    )
}

pub(super) fn delete_component(
    workspace: &mut ProxyWorkspace,
    component_kind: &str,
    component_id: &str,
) -> AppResult<()> {
    let removed = match component_kind {
        "metadata_extractor" => {
            retain_removed(&mut workspace.metadata_extractors, component_id, |item| {
                item.id.to_string()
            })
        }
        "response_assertion" => {
            retain_removed(&mut workspace.response_assertions, component_id, |item| {
                item.id.to_string()
            })
        }
        "fault_preset" => retain_removed(&mut workspace.fault_presets, component_id, |item| {
            item.id.to_string()
        }),
        "certificate_reference" => retain_removed(
            &mut workspace.certificate_references,
            component_id,
            |item| item.id.to_string(),
        ),
        _ => return Err(component_variant_error()),
    };
    if removed {
        Ok(())
    } else {
        Err(AppError::new(
            "WORKSPACE_COMPONENT_NOT_FOUND",
            "Workspace 组件不存在或已被删除。",
        )
        .entity(component_id.to_owned()))
    }
}

pub(super) fn retain_removed<T>(
    items: &mut Vec<T>,
    component_id: &str,
    id: impl Fn(&T) -> String,
) -> bool {
    let before = items.len();
    items.retain(|item| id(item) != component_id);
    items.len() != before
}

pub(super) fn cancelled(message: &str) -> OperationResultViewModel {
    OperationResultViewModel {
        success: false,
        cancelled: true,
        message: message.into(),
        ui_tone: UiTone::Neutral,
        entity_id: None,
        revision: None,
        requires_restart: false,
    }
}

pub(super) fn safe_file_stem(name: &str) -> String {
    let value = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if value.is_empty() {
        "workspace".into()
    } else {
        value
    }
}
