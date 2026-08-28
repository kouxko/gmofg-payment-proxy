//! 协议 Document 规则的结构与 Schema 校验。

use super::{
    DocumentAction, DocumentCondition, MAX_PROTOCOL_DOCUMENT_RULE_ACTIONS,
    MAX_PROTOCOL_DOCUMENT_RULE_BLOB_BYTES, MAX_PROTOCOL_DOCUMENT_RULE_CONDITIONS,
    MAX_PROTOCOL_DOCUMENT_RULE_NAME_BYTES, MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES,
};
use crate::{
    DocumentFieldName, DocumentSchema, DocumentValue, DomainError, ErrorCode,
    MAX_JAVASCRIPT_SAFE_INTEGER, Revision,
};

const MAX_JAVASCRIPT_SAFE_SIGNED_INTEGER: i64 = 9_007_199_254_740_991;

pub(super) fn validate_structure(
    name: &str,
    revision: Revision,
    created_order: u64,
    schema_version: u32,
    conditions: &[DocumentCondition],
    actions: &[DocumentAction],
) -> Result<(), DomainError> {
    let mut error = rule_error("协议 Document 规则结构无效");
    if name.trim().is_empty() || name.len() > MAX_PROTOCOL_DOCUMENT_RULE_NAME_BYTES {
        add_error(&mut error, "name", "规则名称不能为空且不能超过 128 字节");
    }
    if !(1..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(&revision.get()) {
        add_error(
            &mut error,
            "revision",
            "revision 必须在 1 到 JavaScript 安全整数上限之间",
        );
    }
    if !(1..=MAX_JAVASCRIPT_SAFE_INTEGER).contains(&created_order) {
        add_error(
            &mut error,
            "created_order",
            "created_order 必须在 1 到 JavaScript 安全整数上限之间",
        );
    }
    add_content_structure_errors(schema_version, conditions, actions, &mut error);
    finish(error)
}

pub(super) fn validate_content_structure(
    schema_version: u32,
    conditions: &[DocumentCondition],
    actions: &[DocumentAction],
) -> Result<(), DomainError> {
    let mut error = rule_error("协议 Document 规则结构无效");
    add_content_structure_errors(schema_version, conditions, actions, &mut error);
    finish(error)
}

fn add_content_structure_errors(
    schema_version: u32,
    conditions: &[DocumentCondition],
    actions: &[DocumentAction],
    error: &mut DomainError,
) {
    if schema_version == 0 {
        add_error(error, "schema_version", "Schema 版本必须大于 0");
    }
    if conditions.len() > MAX_PROTOCOL_DOCUMENT_RULE_CONDITIONS {
        add_error(error, "conditions", "条件数量不能超过 64 个");
    }
    if actions.is_empty() || actions.len() > MAX_PROTOCOL_DOCUMENT_RULE_ACTIONS {
        add_error(error, "actions", "动作数量必须为 1 到 64 个");
    }
    let mut fields = std::collections::BTreeSet::new();
    for (index, condition) in conditions.iter().enumerate() {
        if !fields.insert(condition.field().as_str()) {
            add_error(
                error,
                format!("conditions.{index}.field"),
                "同一字段不能重复声明条件",
            );
        }
        validate_value_limit(
            condition.value(),
            &format!("conditions.{index}.value"),
            error,
        );
    }
    for (index, action) in actions.iter().enumerate() {
        if let Some((_, value)) = action.field_and_value() {
            validate_value_limit(value, &format!("actions.{index}.value"), error);
        }
    }
}

fn finish(error: DomainError) -> Result<(), DomainError> {
    if error.field_errors.is_empty() {
        Ok(())
    } else {
        Err(error)
    }
}

fn validate_value_limit(value: &DocumentValue, field: &str, error: &mut DomainError) {
    match value {
        DocumentValue::String(value) if value.len() > MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES => {
            add_error(error, field, "文本值不能超过 16 KiB UTF-8 字节");
        }
        DocumentValue::Blob(value) if value.len() > MAX_PROTOCOL_DOCUMENT_RULE_BLOB_BYTES => {
            add_error(error, field, "Blob 值不能超过 64 KiB");
        }
        DocumentValue::Int(value)
            if !(-MAX_JAVASCRIPT_SAFE_SIGNED_INTEGER..=MAX_JAVASCRIPT_SAFE_SIGNED_INTEGER)
                .contains(value) =>
        {
            add_error(error, field, "整数值必须位于 JavaScript 安全整数范围");
        }
        _ => {}
    }
}

pub(super) fn next_rule_revision(current: Revision) -> Result<Revision, DomainError> {
    let next = current.checked_next()?;
    if next.get() > MAX_JAVASCRIPT_SAFE_INTEGER {
        Err(DomainError::new(
            ErrorCode::RevisionConflict,
            "规则 revision 已达到 JavaScript 安全整数上限",
        ))
    } else {
        Ok(next)
    }
}

pub(super) fn validate_field_value(
    schema: &DocumentSchema,
    field: &DocumentFieldName,
    value: &DocumentValue,
    prefix: &str,
    error: &mut DomainError,
) {
    let Some(index) = schema.field_index(field.as_str()) else {
        add_error(
            error,
            format!("{prefix}.field"),
            "字段未在绑定 Schema 中声明",
        );
        return;
    };
    let expected = schema.fields()[index].field_type();
    if expected != value.field_type() {
        add_error(
            error,
            format!("{prefix}.value"),
            format!(
                "需要 {}，实际得到 {}",
                expected.as_str(),
                value.field_type().as_str()
            ),
        );
    }
}

pub(super) fn rule_error(message: &str) -> DomainError {
    DomainError::new(ErrorCode::RuleInvalid, message)
}

pub(super) fn add_error(
    error: &mut DomainError,
    field: impl Into<String>,
    message: impl Into<String>,
) {
    error
        .field_errors
        .entry(field.into())
        .or_default()
        .push(message.into());
}
