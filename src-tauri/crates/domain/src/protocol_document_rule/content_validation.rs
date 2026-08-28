//! 可由旧协议规则与统一规则共同复用的 Document 内容校验。

use super::{DocumentAction, DocumentCondition};
use crate::{DocumentSchema, DomainError};

use super::validation::{add_error, rule_error, validate_content_structure, validate_field_value};

/// 校验统一规则内嵌 Document 内容中不依赖具体 Schema 的资源与结构约束。
pub fn validate_document_rule_content_structure(
    schema_version: u32,
    conditions: &[DocumentCondition],
    actions: &[DocumentAction],
) -> Result<(), DomainError> {
    validate_content_structure(schema_version, conditions, actions)
}

/// 使用协议包编译得到的权威 Schema 校验版本、字段存在性和值类型。
pub fn validate_document_rule_content_against_schema(
    schema_version: u32,
    conditions: &[DocumentCondition],
    actions: &[DocumentAction],
    schema: &DocumentSchema,
) -> Result<(), DomainError> {
    let mut error = rule_error("协议 Document 规则与 Schema 不兼容");
    if schema_version != schema.version() {
        add_error(
            &mut error,
            "schema_version",
            "规则 Schema 版本与绑定 Schema 不一致",
        );
    }
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
        } else if let DocumentAction::ClearField { field } = action
            && schema.field_index(field.as_str()).is_none()
        {
            add_error(
                &mut error,
                format!("actions.{index}.field"),
                "字段未在绑定 Schema 中声明",
            );
        }
    }
    if error.field_errors.is_empty() {
        Ok(())
    } else {
        Err(error)
    }
}
