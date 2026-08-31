use crate::{Condition, ConditionTree, DomainError, ErrorCode, JsonPath, MessageStage};

use super::{
    HttpAction, MAX_THROTTLE_BYTES_PER_SECOND, MAX_TOTAL_DELAY_MS, MAX_TRAFFIC_CHUNK_BYTES,
    MatchField, TerminalAction, TrafficDirection, validate_http_condition,
};

pub(crate) fn validate_http_rule(
    stage: MessageStage,
    condition: &ConditionTree,
    actions: &[HttpAction],
) -> Result<(), DomainError> {
    let mut error = DomainError::new(ErrorCode::RuleInvalid, "规则配置非法");
    if actions.is_empty() {
        error = error.with_field_error("actions", "至少配置一个动作");
    }
    let mut conditions = Vec::new();
    collect_conditions(condition, &mut conditions);
    validate_conditions(&conditions, &mut error);
    validate_total_delay(actions, &mut error);
    validate_actions(stage, actions, &mut error);
    validate_tls_conditions(stage, &conditions, &mut error);

    if error.field_errors.is_empty() {
        Ok(())
    } else {
        Err(error)
    }
}

fn collect_conditions<'a>(tree: &'a ConditionTree, output: &mut Vec<&'a Condition>) {
    match tree {
        ConditionTree::All(children) | ConditionTree::Any(children) => {
            for child in children {
                collect_conditions(child, output);
            }
        }
        ConditionTree::Leaf(condition) => output.push(condition),
    }
}

fn validate_conditions(conditions: &[&Condition], error: &mut DomainError) {
    for (index, condition) in conditions.iter().enumerate() {
        match condition {
            Condition::Http { field, operator } => {
                if let Err(condition_error) = validate_http_condition(field, operator) {
                    for (field, messages) in *condition_error.field_errors {
                        for message in messages {
                            push_field_error(
                                error,
                                format!("conditions.{index}.{field}"),
                                &message,
                            );
                        }
                    }
                }
            }
            Condition::NthHit { count: 0 } => push_field_error(
                error,
                format!("conditions.{index}.nth_hit"),
                "第 N 次命中必须大于 0",
            ),
            Condition::NthHit { .. }
            | Condition::Document { .. }
            | Condition::DocumentPattern { .. } => {}
        }
    }
}

fn validate_total_delay(actions: &[HttpAction], error: &mut DomainError) {
    let total_delay = actions.iter().fold(0_u64, |total, action| {
        total.saturating_add(match action {
            HttpAction::Delay { milliseconds } => *milliseconds,
            HttpAction::Jitter {
                maximum_milliseconds,
                ..
            } => *maximum_milliseconds,
            _ => 0,
        })
    });
    if total_delay > MAX_TOTAL_DELAY_MS {
        push_field_error(error, "actions", "累计延迟不得超过 600000 毫秒");
    }
}

fn validate_tls_conditions(
    stage: MessageStage,
    conditions: &[&Condition],
    error: &mut DomainError,
) {
    if stage == MessageStage::TlsHandshake
        && conditions.iter().any(|condition| {
            !matches!(
                condition,
                Condition::Http {
                    field: MatchField::CertificateFingerprint,
                    ..
                } | Condition::NthHit { .. }
            )
        })
    {
        push_field_error(
            error,
            "conditions",
            "TLS 握手拒绝只允许通道、客户端证书和第 N 次命中条件",
        );
    }
}

fn validate_actions(stage: MessageStage, actions: &[HttpAction], error: &mut DomainError) {
    let terminal_positions = actions
        .iter()
        .enumerate()
        .filter_map(|(index, action)| action.is_terminal().then_some(index))
        .collect::<Vec<_>>();
    if terminal_positions.len() > 1
        || terminal_positions
            .first()
            .is_some_and(|&index| index + 1 != actions.len())
    {
        push_field_error(error, "actions", "终止动作必须唯一且位于动作列表末尾");
    }

    for (index, action) in actions.iter().enumerate() {
        validate_action_compatibility(stage, error, index, action);
        validate_action_limits(error, index, action);
        validate_action_content(error, index, action);
    }
}

fn validate_action_compatibility(
    stage: MessageStage,
    error: &mut DomainError,
    index: usize,
    action: &HttpAction,
) {
    if !action_compatible(stage, action) {
        push_field_error(error, format!("actions.{index}"), "动作与规则阶段不兼容");
    }
    if let HttpAction::CustomHttpStatus { status } = action
        && !(100..=599).contains(status)
    {
        push_field_error(
            error,
            format!("actions.{index}.status"),
            "HTTP 状态码必须位于 100..599",
        );
    }
}

fn validate_action_limits(error: &mut DomainError, index: usize, action: &HttpAction) {
    match action {
        HttpAction::Delay { milliseconds } if *milliseconds == 0 => push_field_error(
            error,
            format!("actions.{index}.milliseconds"),
            "延迟必须大于 0 毫秒",
        ),
        HttpAction::Jitter {
            minimum_milliseconds,
            maximum_milliseconds,
            ..
        } => {
            if minimum_milliseconds > maximum_milliseconds {
                push_field_error(
                    error,
                    format!("actions.{index}.minimum_milliseconds"),
                    "最小抖动不得大于最大抖动",
                );
            }
            if *maximum_milliseconds > MAX_TOTAL_DELAY_MS {
                push_field_error(
                    error,
                    format!("actions.{index}.maximum_milliseconds"),
                    "单次抖动不得超过 600000 毫秒",
                );
            }
        }
        HttpAction::Throttle {
            bytes_per_second,
            chunk_bytes,
            ..
        } => {
            if !(1..=MAX_THROTTLE_BYTES_PER_SECOND).contains(bytes_per_second) {
                push_field_error(
                    error,
                    format!("actions.{index}.bytes_per_second"),
                    "限速必须位于 1..104857600 B/s",
                );
            }
            if !(1..=MAX_TRAFFIC_CHUNK_BYTES).contains(chunk_bytes) {
                push_field_error(
                    error,
                    format!("actions.{index}.chunk_bytes"),
                    "分块大小必须位于 1..1048576 字节",
                );
            }
        }
        HttpAction::Intermittent {
            available_milliseconds,
            blocked_milliseconds,
            ..
        } => {
            validate_window(
                error,
                index,
                "available_milliseconds",
                *available_milliseconds,
            );
            validate_window(error, index, "blocked_milliseconds", *blocked_milliseconds);
        }
        _ => {}
    }

    match action {
        HttpAction::Terminal(TerminalAction::IncorrectContentLength { delta }) if *delta == 0 => {
            push_field_error(
                error,
                format!("actions.{index}.delta"),
                "错误长度差值不能为 0",
            );
        }
        HttpAction::Terminal(
            TerminalAction::UpstreamConnectTimeout { milliseconds }
            | TerminalAction::UpstreamWriteTimeout { milliseconds }
            | TerminalAction::UpstreamReadTimeout { milliseconds },
        ) if *milliseconds == 0 => push_field_error(
            error,
            format!("actions.{index}.milliseconds"),
            "故障超时必须大于 0 毫秒",
        ),
        HttpAction::Terminal(TerminalAction::MockResponse { status, .. })
            if !(100..=599).contains(status) =>
        {
            push_field_error(
                error,
                format!("actions.{index}.status"),
                "Mock HTTP 状态码必须位于 100..599",
            );
        }
        _ => {}
    }
}

fn validate_window(error: &mut DomainError, index: usize, field: &str, value: u64) {
    if !(1..=MAX_TOTAL_DELAY_MS).contains(&value) {
        let message = if field == "available_milliseconds" {
            "可用窗口必须位于 1..600000 毫秒"
        } else {
            "阻断窗口必须位于 1..600000 毫秒"
        };
        push_field_error(error, format!("actions.{index}.{field}"), message);
    }
}

fn validate_action_content(error: &mut DomainError, index: usize, action: &HttpAction) {
    match action {
        HttpAction::SetJsonField { path, .. } if JsonPath::parse(path).is_err() => {
            push_field_error(error, format!("actions.{index}.path"), "JSON 字段路径非法");
        }
        HttpAction::SetHeader { name, value } => {
            validate_header(error, &format!("actions.{index}"), name, value);
        }
        HttpAction::Terminal(TerminalAction::MockResponse { headers, .. }) => {
            for (header_index, (name, value)) in headers.iter().enumerate() {
                validate_header(
                    error,
                    &format!("actions.{index}.headers.{header_index}"),
                    name,
                    value,
                );
            }
        }
        _ => {}
    }
}

fn validate_header(error: &mut DomainError, field_prefix: &str, name: &str, value: &str) {
    if !is_valid_header_name(name) {
        push_field_error(
            error,
            format!("{field_prefix}.name"),
            "Header 名称必须是非空 ASCII token",
        );
    }
    if !is_valid_header_value(value) {
        push_field_error(
            error,
            format!("{field_prefix}.value"),
            "Header 值不能包含换行、NUL 或其他非法控制字符",
        );
    }
    if matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "content-length"
            | "transfer-encoding"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "upgrade"
            | "te"
            | "trailer"
    ) {
        push_field_error(
            error,
            format!("{field_prefix}.name"),
            "该 Header 由 Rust 转发管线统一管理，规则不得直接设置",
        );
    }
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_valid_header_value(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte == b'\t' || byte >= 0x20 && byte != 0x7f)
}

fn push_field_error(error: &mut DomainError, field: impl Into<String>, message: impl Into<String>) {
    error
        .field_errors
        .entry(field.into())
        .or_default()
        .push(message.into());
}

fn action_compatible(stage: MessageStage, action: &HttpAction) -> bool {
    match action {
        HttpAction::SetJsonField { .. }
        | HttpAction::ReplaceBodyText(_)
        | HttpAction::SetHeader { .. }
        | HttpAction::Delay { .. }
        | HttpAction::Jitter { .. }
        | HttpAction::Pause => stage != MessageStage::TlsHandshake,
        HttpAction::Throttle { direction, .. } | HttpAction::Intermittent { direction, .. } => {
            matches!(
                (stage, direction),
                (MessageStage::Request, TrafficDirection::Upstream)
                    | (MessageStage::Response, TrafficDirection::Downstream)
            )
        }
        HttpAction::CustomHttpStatus { .. } => stage == MessageStage::Response,
        HttpAction::Terminal(terminal) => terminal_compatible(stage, terminal),
    }
}

fn terminal_compatible(stage: MessageStage, terminal: &TerminalAction) -> bool {
    match terminal {
        TerminalAction::RejectTlsHandshake => stage == MessageStage::TlsHandshake,
        TerminalAction::DisconnectBeforeUpstream
        | TerminalAction::UpstreamConnectTimeout { .. }
        | TerminalAction::UpstreamWriteTimeout { .. }
        | TerminalAction::UpstreamReadTimeout { .. }
        | TerminalAction::DropUpstreamResponse { .. }
        | TerminalAction::MockResponse { .. }
        | TerminalAction::DisconnectDuringUpstreamWrite { .. } => stage == MessageStage::Request,
        TerminalAction::InvalidJson { .. }
        | TerminalAction::IncorrectContentLength { .. }
        | TerminalAction::TruncateResponse { .. }
        | TerminalAction::DisconnectDuringDownstreamWrite { .. } => stage == MessageStage::Response,
    }
}
