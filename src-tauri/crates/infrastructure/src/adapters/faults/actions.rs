use super::{
    AppError, AppResult, BTreeMap, BodyCodec, DropResponseMode, FaultParameterValue,
    FaultParameters, HttpAction, JitterScope, MessageStage, TerminalAction, TrafficDirection,
    Value,
};

#[allow(clippy::unnecessary_wraps)]
pub(super) fn disconnect(_: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
    ))
}

pub(super) fn request_delay(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((MessageStage::Request, delay(values)?))
}

pub(super) fn response_delay(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((MessageStage::Response, delay(values)?))
}

pub(super) fn delay(values: &FaultParameters) -> AppResult<HttpAction> {
    Ok(HttpAction::Delay {
        milliseconds: u64_parameter(values, "milliseconds")?,
    })
}

pub(super) fn modify_json(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    let value_text = json_parameter(values, "value")?;
    let value = serde_json::from_str(value_text).map_err(|error| {
        parameter_error("value", format!("参数 value 必须包含合法 JSON：{error}"))
    })?;
    Ok((
        MessageStage::Request,
        HttpAction::SetJsonField {
            path: text_parameter(values, "path")?.to_owned(),
            value,
        },
    ))
}

pub(super) fn drop_response(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    let mode = if boolean_parameter(values, "close_after_request_write")? {
        DropResponseMode::CloseAfterRequestWrite
    } else {
        DropResponseMode::ReadCompleteResponse
    };
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::DropUpstreamResponse { mode }),
    ))
}

pub(super) fn connect_timeout(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::UpstreamConnectTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

pub(super) fn write_timeout(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::UpstreamWriteTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

pub(super) fn read_timeout(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::UpstreamReadTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

pub(super) fn custom_status(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    let status = status_parameter(values)?;
    Ok((
        MessageStage::Response,
        HttpAction::CustomHttpStatus { status },
    ))
}

pub(super) fn mock_response(
    values: &FaultParameters,
    body_codec: &dyn BodyCodec,
) -> AppResult<(MessageStage, HttpAction)> {
    let status = status_parameter(values)?;
    let body = json_parameter(values, "body")?;
    serde_json::from_str::<Value>(body)
        .map_err(|error| parameter_error("body", format!("Mock Body 不是合法 JSON：{error}")))?;
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::MockResponse {
            status,
            headers: Vec::new(),
            body_bytes: encode_body(body_codec, body)?,
        }),
    ))
}

pub(super) fn invalid_json(
    values: &FaultParameters,
    body_codec: &dyn BodyCodec,
) -> AppResult<(MessageStage, HttpAction)> {
    let body = text_parameter(values, "body")?;
    if serde_json::from_str::<Value>(body).is_ok() {
        return Err(parameter_error(
            "body",
            "非法 JSON 模板的 Body 必须保持语法非法。",
        ));
    }
    Ok((
        MessageStage::Response,
        HttpAction::Terminal(TerminalAction::InvalidJson {
            body_bytes: encode_body(body_codec, body)?,
        }),
    ))
}

pub(super) fn wrong_length(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    let delta = integer_parameter(values, "delta")?;
    Ok((
        MessageStage::Response,
        HttpAction::Terminal(TerminalAction::IncorrectContentLength { delta }),
    ))
}

pub(super) fn truncate(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Response,
        HttpAction::Terminal(TerminalAction::TruncateResponse {
            bytes: u64_parameter(values, "bytes")?,
        }),
    ))
}

pub(super) fn throttle_upstream(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    throttle(values, MessageStage::Request, TrafficDirection::Upstream)
}

pub(super) fn throttle_downstream(
    values: &FaultParameters,
) -> AppResult<(MessageStage, HttpAction)> {
    throttle(values, MessageStage::Response, TrafficDirection::Downstream)
}

pub(super) fn throttle(
    values: &FaultParameters,
    stage: MessageStage,
    direction: TrafficDirection,
) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        stage,
        HttpAction::Throttle {
            bytes_per_second: u64_parameter(values, "bytes_per_second")?,
            chunk_bytes: u64_parameter(values, "chunk_bytes")?,
            direction,
        },
    ))
}

pub(super) fn jitter_upstream(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    jitter(values, MessageStage::Request)
}

pub(super) fn jitter_downstream(values: &FaultParameters) -> AppResult<(MessageStage, HttpAction)> {
    jitter(values, MessageStage::Response)
}

pub(super) fn jitter(
    values: &FaultParameters,
    stage: MessageStage,
) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        stage,
        HttpAction::Jitter {
            minimum_milliseconds: u64_parameter(values, "minimum_milliseconds")?,
            maximum_milliseconds: u64_parameter(values, "maximum_milliseconds")?,
            scope: if boolean_parameter(values, "per_chunk")? {
                JitterScope::PerChunk
            } else {
                JitterScope::BeforeMessage
            },
        },
    ))
}

pub(super) fn intermittent_upstream(
    values: &FaultParameters,
) -> AppResult<(MessageStage, HttpAction)> {
    intermittent(values, MessageStage::Request, TrafficDirection::Upstream)
}

pub(super) fn intermittent_downstream(
    values: &FaultParameters,
) -> AppResult<(MessageStage, HttpAction)> {
    intermittent(values, MessageStage::Response, TrafficDirection::Downstream)
}

pub(super) fn intermittent(
    values: &FaultParameters,
    stage: MessageStage,
    direction: TrafficDirection,
) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        stage,
        HttpAction::Intermittent {
            available_milliseconds: u64_parameter(values, "available_milliseconds")?,
            blocked_milliseconds: u64_parameter(values, "blocked_milliseconds")?,
            direction,
        },
    ))
}

pub(super) fn disconnect_upstream_mid_body(
    values: &FaultParameters,
) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Request,
        HttpAction::Terminal(TerminalAction::DisconnectDuringUpstreamWrite {
            after_bytes: u64_parameter(values, "after_bytes")?,
        }),
    ))
}

pub(super) fn disconnect_downstream_mid_body(
    values: &FaultParameters,
) -> AppResult<(MessageStage, HttpAction)> {
    Ok((
        MessageStage::Response,
        HttpAction::Terminal(TerminalAction::DisconnectDuringDownstreamWrite {
            after_bytes: u64_parameter(values, "after_bytes")?,
        }),
    ))
}

pub(super) fn status_parameter(values: &FaultParameters) -> AppResult<u16> {
    let status = integer_parameter(values, "status")?;
    if !(100..=599).contains(&status) {
        return Err(parameter_error(
            "status",
            "参数 status 必须是 100 到 599 之间的整数。",
        ));
    }
    u16::try_from(status).map_err(|_| parameter_error("status", "HTTP 状态码超出范围。"))
}

pub(super) fn u64_parameter(values: &FaultParameters, name: &str) -> AppResult<u64> {
    let value = integer_parameter(values, name)?;
    u64::try_from(value).map_err(|_| parameter_error(name, format!("参数 {name} 必须是非负整数。")))
}

pub(super) fn integer_parameter(values: &FaultParameters, name: &str) -> AppResult<i64> {
    match values.get(name) {
        Some(FaultParameterValue::Integer(value)) => Ok(*value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是整数。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

pub(super) fn boolean_parameter(values: &FaultParameters, name: &str) -> AppResult<bool> {
    match values.get(name) {
        Some(FaultParameterValue::Boolean(value)) => Ok(*value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是布尔值。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

pub(super) fn text_parameter<'a>(values: &'a FaultParameters, name: &str) -> AppResult<&'a str> {
    match values.get(name) {
        Some(FaultParameterValue::Text(value)) => Ok(value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是文本。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

pub(super) fn json_parameter<'a>(values: &'a FaultParameters, name: &str) -> AppResult<&'a str> {
    match values.get(name) {
        Some(FaultParameterValue::Json(value)) => Ok(value),
        Some(_) => Err(parameter_error(
            name,
            format!("参数 {name} 必须是 JSON 文本。"),
        )),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

pub(super) fn parameter_error(name: &str, message: impl Into<String>) -> AppError {
    AppError::field(
        "RULE_INVALID",
        "故障参数无效。",
        BTreeMap::from([(format!("parameters.{name}"), vec![message.into()])]),
    )
}

pub(super) fn encode_body(body_codec: &dyn BodyCodec, text: &str) -> AppResult<Vec<u8>> {
    body_codec
        .encode(text)
        .map_err(|error| AppError::new(error.code, error.message))
}
