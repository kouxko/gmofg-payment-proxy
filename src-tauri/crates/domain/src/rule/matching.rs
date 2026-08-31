use crate::{DomainError, ErrorCode, JsonPointer};

use super::{MatchContext, MatchField, MatchOperator};

pub(super) fn matches_condition(
    field: &MatchField,
    operator: &MatchOperator,
    context: &MatchContext<'_>,
) -> Result<bool, String> {
    let value = match field {
        MatchField::TerminalIp => context.terminal.source_ip.clone(),
        MatchField::CertificateFingerprint => context.terminal.certificate_sha256.clone(),
        MatchField::Method => context
            .method
            .ok_or_else(|| "Method 匹配缺少关联请求元数据".to_owned())?
            .to_owned(),
        MatchField::RequestTarget => context
            .request_target
            .ok_or_else(|| "RequestTarget 匹配缺少关联请求元数据".to_owned())?
            .to_owned(),
        MatchField::Header(path) => {
            let pointer = header_pointer(path)?;
            let name = pointer.tokens()[0].as_bytes();
            return Ok(context
                .headers
                .iter()
                .filter(|header| header.name.eq_ignore_ascii_case(name))
                .any(|header| matches_bytes(operator, header.value)));
        }
    };
    Ok(matches_string(field, operator, &value))
}

fn matches_string(field: &MatchField, operator: &MatchOperator, value: &str) -> bool {
    match operator {
        MatchOperator::Equals(expected) => value == *expected,
        MatchOperator::Contains(fragment) => value.contains(fragment),
        MatchOperator::StartsWith(prefix) => value.starts_with(prefix),
        MatchOperator::EndsWith(suffix) => value.ends_with(suffix),
        MatchOperator::Wildcard(pattern) => wildcard_matches(
            pattern.as_bytes(),
            value.as_bytes(),
            matches!(field, MatchField::RequestTarget).then_some(&b"/?"[..]),
        ),
    }
}

fn matches_bytes(operator: &MatchOperator, value: &[u8]) -> bool {
    match operator {
        MatchOperator::Equals(expected) => value == expected.as_bytes(),
        MatchOperator::Contains(fragment) => {
            fragment.is_empty()
                || value
                    .windows(fragment.len())
                    .any(|window| window == fragment.as_bytes())
        }
        MatchOperator::StartsWith(prefix) => value.starts_with(prefix.as_bytes()),
        MatchOperator::EndsWith(suffix) => value.ends_with(suffix.as_bytes()),
        MatchOperator::Wildcard(pattern) => wildcard_matches(pattern.as_bytes(), value, None),
    }
}

fn wildcard_matches(pattern: &[u8], value: &[u8], separators: Option<&[u8]>) -> bool {
    let mut previous = vec![false; value.len() + 1];
    let mut current = vec![false; value.len() + 1];
    previous[0] = true;
    for &pattern_byte in pattern {
        current.fill(false);
        if pattern_byte == b'*' {
            current[0] = previous[0];
            for index in 1..=value.len() {
                let can_consume = separators.is_none_or(|items| !items.contains(&value[index - 1]));
                current[index] = previous[index] || (can_consume && current[index - 1]);
            }
        } else {
            for index in 1..=value.len() {
                current[index] = previous[index - 1] && pattern_byte == value[index - 1];
            }
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[value.len()]
}

fn header_pointer(path: &str) -> Result<JsonPointer, String> {
    let pointer = JsonPointer::parse(path).map_err(|_| "Header 名称路径必须是 /name".to_owned())?;
    if pointer.tokens().len() != 1 || pointer.tokens()[0].is_empty() {
        return Err("Header 名称路径必须只包含一个非空层级".into());
    }
    if !pointer.tokens()[0]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    {
        return Err("Header 名称必须是 ASCII HTTP token".into());
    }
    Ok(pointer)
}

pub fn matches_http_condition(
    field: &MatchField,
    operator: &MatchOperator,
    context: &MatchContext<'_>,
) -> Result<bool, DomainError> {
    matches_condition(field, operator, context).map_err(|message| {
        DomainError::new(ErrorCode::RuleInvalid, "HTTP规则条件运行时匹配失败")
            .with_field_error("condition", message)
    })
}

pub fn validate_http_condition(
    field: &MatchField,
    operator: &MatchOperator,
) -> Result<(), DomainError> {
    let invalid = match field {
        MatchField::Method if !matches!(operator, MatchOperator::Equals(_)) => {
            Some("Method 只支持精确匹配")
        }
        MatchField::Header(path) if header_pointer(path).is_err() => {
            Some("Header 名称路径必须是单层 /name")
        }
        _ => None,
    };
    if let Some(message) = invalid {
        return Err(DomainError::new(ErrorCode::RuleInvalid, "HTTP规则条件无效")
            .with_field_error("condition", message));
    }
    Ok(())
}
