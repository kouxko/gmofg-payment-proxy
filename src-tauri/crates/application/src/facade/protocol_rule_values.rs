//! 协议报文规则编辑器的文本到类型化 `DocumentValue` 解析边界。
//!
//! `WebView` 只提交字段类型与原始编辑文本；Rust 按当前 recursive JSON value 合同解析并
//! 校验目标类型。错误不会拼接原始文本，避免用户输入进入日志。

use std::collections::BTreeMap;

use intercept_proxy_domain::{DocumentValue, MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES};

use crate::{AppError, AppResult, ProtocolPackageSchemaFieldTypeViewModel};

/// 把 协议报文规则编辑文本解析为 Schema 字段类型对应的领域值。
///
/// String 保留原始内容；Number、Boolean、Object 和 Array 使用标准 JSON 解析，并由
/// Domain 的 recursive Document 与 JavaScript Number 合同拒绝无效值。
pub fn parse_protocol_rule_value(
    field_type: ProtocolPackageSchemaFieldTypeViewModel,
    raw: &str,
) -> AppResult<DocumentValue> {
    match field_type {
        ProtocolPackageSchemaFieldTypeViewModel::String => parse_string(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Number => parse_number(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Boolean => parse_bool(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Object => parse_composite(raw, true),
        ProtocolPackageSchemaFieldTypeViewModel::Array => parse_composite(raw, false),
    }
}

fn parse_string(raw: &str) -> AppResult<DocumentValue> {
    if raw.len() > MAX_PROTOCOL_DOCUMENT_RULE_STRING_BYTES {
        return Err(value_too_large("文本值不能超过 16 KiB UTF-8 字节。"));
    }
    Ok(DocumentValue::String(raw.to_owned()))
}

fn parse_number(raw: &str) -> AppResult<DocumentValue> {
    let document = intercept_proxy_domain::Document::parse_json(raw)?;
    match document.root() {
        DocumentValue::Number(value) => Ok(DocumentValue::Number(*value)),
        _ => Err(value_invalid("请输入标准JSON number。")),
    }
}

fn parse_bool(raw: &str) -> AppResult<DocumentValue> {
    let document = intercept_proxy_domain::Document::parse_json(raw)?;
    match document.root() {
        DocumentValue::Boolean(value) => Ok(DocumentValue::Boolean(*value)),
        _ => Err(value_invalid("请输入标准JSON boolean。")),
    }
}

fn parse_composite(raw: &str, object: bool) -> AppResult<DocumentValue> {
    let document = intercept_proxy_domain::Document::parse_json(raw)?;
    match (object, document.root()) {
        (true, DocumentValue::Object(value)) => Ok(DocumentValue::Object(value.clone())),
        (false, DocumentValue::Array(value)) => Ok(DocumentValue::Array(value.clone())),
        _ => Err(value_invalid("值必须匹配Schema声明的JSON容器类型。")),
    }
}

fn value_invalid(detail: &'static str) -> AppError {
    value_error(
        "PROTOCOL_RULE_VALUE_INVALID",
        "协议报文规则值格式无效。",
        detail,
    )
}

fn value_too_large(detail: &'static str) -> AppError {
    value_error(
        "PROTOCOL_RULE_VALUE_TOO_LARGE",
        "协议报文规则值超过安全上限。",
        detail,
    )
}

fn value_error(code: &'static str, message: &'static str, detail: &'static str) -> AppError {
    AppError::field(
        code,
        message,
        BTreeMap::from([("raw".into(), vec![detail.into()])]),
    )
}
