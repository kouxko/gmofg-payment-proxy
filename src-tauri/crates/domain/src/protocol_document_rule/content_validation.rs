//! 可由旧协议规则与统一规则共同复用的 Document 内容校验。

use super::{ProtocolDocumentOperation, ProtocolDocumentPredicate};
use crate::{DocumentSchemaNode, DomainError};

use super::validation::{rule_error, validate_content_structure, validate_field_value};

/// 校验统一规则内嵌 Document 内容中不依赖具体 Schema 的资源与结构约束。
pub fn validate_document_rule_content_structure(
    conditions: &[ProtocolDocumentPredicate],
    actions: &[ProtocolDocumentOperation],
) -> Result<(), DomainError> {
    validate_content_structure(conditions, actions)
}

/// 对 Schema 已声明路径校验值类型；未声明路径保留为规则自身的路径合同。
pub fn validate_document_rule_content_against_schema(
    conditions: &[ProtocolDocumentPredicate],
    actions: &[ProtocolDocumentOperation],
    schema: &DocumentSchemaNode,
) -> Result<(), DomainError> {
    let mut error = rule_error("协议 Document 规则与 Schema 不兼容");
    for (index, condition) in conditions.iter().enumerate() {
        validate_field_value(
            schema,
            condition.field(),
            condition.value(),
            &format!("conditions.{index}"),
            &mut error,
        );
    }
    for (index, action) in actions.iter().enumerate() {
        if let Some((field, value)) = action.field_and_value() {
            validate_field_value(
                schema,
                field,
                value,
                &format!("actions.{index}"),
                &mut error,
            );
        }
    }
    if error.field_errors.is_empty() {
        Ok(())
    } else {
        Err(error)
    }
}
