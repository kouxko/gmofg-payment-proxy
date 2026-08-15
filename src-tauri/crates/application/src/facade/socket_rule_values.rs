//! Socket 规则编辑器的文本到类型化 `DocumentValue` 解析边界。
//!
//! `WebView` 只提交字段类型与原始编辑文本；UTF-8 字节限制、JavaScript 安全整数、Bool
//! 字面量和 Blob Hex 均由 Rust 统一解释。错误不会拼接原始文本，避免用户输入进入日志。

use std::collections::BTreeMap;

use intercept_proxy_domain::{
    DocumentValue, MAX_JAVASCRIPT_SAFE_INTEGER, MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES,
    MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES,
};

use crate::{AppError, AppResult, ProtocolPackageSchemaFieldTypeViewModel};

const MAX_BLOB_TEXT_BYTES: usize = MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES * 4;
/// Int 的合法文本最多只有 17 个字节；保留 128 字节预算允许常见的外围空白，同时在
/// `trim`、逐字节语法检查和 `i64` 解析前拒绝无意义的超长 IPC 输入。
const MAX_SOCKET_RULE_INT_TEXT_BYTES: usize = 128;

/// 把 Socket 规则编辑文本解析为 Schema 字段类型对应的领域值。
///
/// String 保留原始内容；Int 只接受十进制整数；Bool 只接受精确 `true`/`false`；Blob
/// 接受十六进制数字以及 Unicode 空白字符、冒号、连字符分隔符。所有解码后值上限与
/// Domain 保存门禁一致，文本表示另有进入解析器前的扫描预算。
pub fn parse_socket_rule_value(
    field_type: ProtocolPackageSchemaFieldTypeViewModel,
    raw: &str,
) -> AppResult<DocumentValue> {
    match field_type {
        ProtocolPackageSchemaFieldTypeViewModel::String => parse_string(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Int => parse_int(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Bool => parse_bool(raw),
        ProtocolPackageSchemaFieldTypeViewModel::Blob => parse_blob(raw),
    }
}

fn parse_string(raw: &str) -> AppResult<DocumentValue> {
    if raw.len() > MAX_SOCKET_DOCUMENT_RULE_STRING_BYTES {
        return Err(value_too_large("文本值不能超过 16 KiB UTF-8 字节。"));
    }
    Ok(DocumentValue::String(raw.to_owned()))
}

fn parse_int(raw: &str) -> AppResult<DocumentValue> {
    if raw.len() > MAX_SOCKET_RULE_INT_TEXT_BYTES {
        return Err(value_too_large("整数文本不能超过 128 个 UTF-8 字节。"));
    }
    let value = raw.trim();
    if !is_decimal_integer(value) {
        return Err(value_invalid("请输入不带小数点或指数的十进制整数。"));
    }
    let parsed = value
        .parse::<i64>()
        .map_err(|_| value_invalid("整数必须位于 JavaScript 安全整数范围内。"))?;
    let maximum = i64::try_from(MAX_JAVASCRIPT_SAFE_INTEGER)
        .expect("JavaScript safe integer constant must fit i64");
    if !(-maximum..=maximum).contains(&parsed) {
        return Err(value_invalid("整数必须位于 JavaScript 安全整数范围内。"));
    }
    Ok(DocumentValue::Int(parsed))
}

fn is_decimal_integer(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_bool(raw: &str) -> AppResult<DocumentValue> {
    match raw {
        "true" => Ok(DocumentValue::Bool(true)),
        "false" => Ok(DocumentValue::Bool(false)),
        _ => Err(value_invalid("布尔值只能是小写字面量 true 或 false。")),
    }
}

fn parse_blob(raw: &str) -> AppResult<DocumentValue> {
    // IPC 已经拥有 raw String，但仍限制扫描成本；4 倍预算足以表示每字节两位 Hex 与常见
    // 空白/冒号/连字符分隔格式，且不会为了去分隔符再创建完整紧凑副本。
    if raw.len() > MAX_BLOB_TEXT_BYTES {
        return Err(value_too_large(
            "Blob 文本表示过长，解码结果不能超过 64 KiB。",
        ));
    }
    let mut bytes = Vec::new();
    let mut high_nibble = None;
    for character in raw.chars() {
        if character.is_whitespace() || matches!(character, ':' | '-') {
            continue;
        }
        let Some(nibble) = hex_nibble(character) else {
            return Err(value_invalid(
                "Blob 只能包含十六进制数字、Unicode 空白字符、冒号或连字符。",
            ));
        };
        if let Some(high) = high_nibble.take() {
            if bytes.len() == MAX_SOCKET_DOCUMENT_RULE_BLOB_BYTES {
                return Err(value_too_large("Blob 值不能超过 64 KiB。"));
            }
            bytes.push((high << 4) | nibble);
        } else {
            high_nibble = Some(nibble);
        }
    }
    if high_nibble.is_some() {
        return Err(value_invalid("Blob 的十六进制数字必须成对出现。"));
    }
    Ok(DocumentValue::Blob(bytes))
}

const fn hex_nibble(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(character as u8 - b'0'),
        'a'..='f' => Some(character as u8 - b'a' + 10),
        'A'..='F' => Some(character as u8 - b'A' + 10),
        _ => None,
    }
}

fn value_invalid(detail: &'static str) -> AppError {
    value_error(
        "SOCKET_RULE_VALUE_INVALID",
        "Socket 规则值格式无效。",
        detail,
    )
}

fn value_too_large(detail: &'static str) -> AppError {
    value_error(
        "SOCKET_RULE_VALUE_TOO_LARGE",
        "Socket 规则值超过安全上限。",
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
