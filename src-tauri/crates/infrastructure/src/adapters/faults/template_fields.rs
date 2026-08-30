use super::{
    AppResult, BTreeMap, BodyCodec, FaultParameterFieldViewModel, FaultParameterKind,
    FaultParameterValue, FaultParameters, FaultTemplateViewModel, HttpAction, MessageStage,
    TemplateAction, TemplateDefinition, UiTone,
};

pub(super) fn template(
    id: &str,
    name: &str,
    stage: &str,
    behavior: &str,
    affected: &str,
    risk: &str,
    action: fn(&FaultParameters) -> AppResult<(MessageStage, HttpAction)>,
) -> TemplateDefinition {
    let (default_parameters, parameter_schema) = parameter_definitions(id);
    TemplateDefinition {
        view: FaultTemplateViewModel {
            template_id: id.into(),
            name: name.into(),
            stage_text: stage.into(),
            behavior_text: behavior.into(),
            affected_party_text: affected.into(),
            default_channel: intercept_proxy_domain::ChannelId::new("default")
                .expect("generic placeholder channel"),
            default_nth_hit: 1,
            default_one_shot: false,
            default_priority: 100,
            default_parameters,
            parameter_schema,
            risk_text: risk.into(),
            ui_tone: if risk == "高" {
                UiTone::Danger
            } else {
                UiTone::Warning
            },
        },
        action: TemplateAction::Plain(action),
    }
}

pub(super) fn encoded_template(
    id: &str,
    name: &str,
    stage: &str,
    behavior: &str,
    affected: &str,
    risk: &str,
    action: fn(&FaultParameters, &dyn BodyCodec) -> AppResult<(MessageStage, HttpAction)>,
) -> TemplateDefinition {
    let (default_parameters, parameter_schema) = parameter_definitions(id);
    TemplateDefinition {
        view: FaultTemplateViewModel {
            template_id: id.into(),
            name: name.into(),
            stage_text: stage.into(),
            behavior_text: behavior.into(),
            affected_party_text: affected.into(),
            default_channel: intercept_proxy_domain::ChannelId::new("default")
                .expect("generic placeholder channel"),
            default_nth_hit: 1,
            default_one_shot: false,
            default_priority: 100,
            default_parameters,
            parameter_schema,
            risk_text: risk.into(),
            ui_tone: if risk == "高" {
                UiTone::Danger
            } else {
                UiTone::Warning
            },
        },
        action: TemplateAction::Encoded(action),
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn parameter_definitions(
    template_id: &str,
) -> (
    BTreeMap<String, FaultParameterValue>,
    Vec<FaultParameterFieldViewModel>,
) {
    match template_id {
        "request_delay" | "response_delay" => one_integer(
            "milliseconds",
            "延迟时间（毫秒）",
            "命中后等待的精确时长。",
            70_000,
            0,
            i64::MAX,
        ),
        "upstream_connect_timeout" | "upstream_write_timeout" | "upstream_read_timeout" => {
            one_integer(
                "milliseconds",
                "超时时间（毫秒）",
                "保持对应网络阶段直至此时长结束。",
                70_000,
                1,
                i64::MAX,
            )
        }
        "modify_request_json" => (
            BTreeMap::from([
                ("path".into(), FaultParameterValue::Text("$.result".into())),
                ("value".into(), FaultParameterValue::Json("null".into())),
            ]),
            vec![
                text_field("path", "JSON 路径", "要修改的 JSON 字段路径。", false),
                json_field("value", "JSON 值", "写入字段的合法 JSON 值。"),
            ],
        ),
        "drop_upstream_response" => (
            BTreeMap::from([(
                "close_after_request_write".into(),
                FaultParameterValue::Boolean(false),
            )]),
            vec![boolean_field(
                "close_after_request_write",
                "写完请求后立即断开",
                "关闭时不读取上游响应；关闭则完整读取后丢弃响应。",
            )],
        ),
        "custom_http_status" => one_integer(
            "status",
            "HTTP 状态码",
            "返回给客户端的 HTTP 状态码。",
            500,
            100,
            599,
        ),
        "mock_json" => (
            BTreeMap::from([
                ("status".into(), FaultParameterValue::Integer(200)),
                ("body".into(), FaultParameterValue::Json("{}".into())),
            ]),
            vec![
                integer_field(
                    "status",
                    "HTTP 状态码",
                    "Mock 响应的 HTTP 状态码。",
                    100,
                    599,
                ),
                json_field(
                    "body",
                    "JSON Body",
                    "必须是合法 JSON，且所有字符必须可由当前产品编解码器无损编码。",
                ),
            ],
        ),
        "invalid_json" => (
            BTreeMap::from([("body".into(), FaultParameterValue::Text("{invalid".into()))]),
            vec![multiline_text_field(
                "body",
                "非法 JSON Body",
                "必须保持 JSON 语法非法，且所有字符必须可由当前产品编解码器无损编码。",
            )],
        ),
        "wrong_content_length" => one_integer(
            "delta",
            "长度偏移量",
            "声明的 Content-Length 相对真实 Body 长度的有符号偏移。",
            1,
            i64::MIN,
            i64::MAX,
        ),
        "truncate_response" => one_integer(
            "bytes",
            "发送字节数",
            "仅发送响应 Body 的前 N 字节后断开。",
            1,
            0,
            i64::MAX,
        ),
        "throttle_upstream" | "throttle_downstream" => (
            BTreeMap::from([
                (
                    "bytes_per_second".into(),
                    FaultParameterValue::Integer(1024),
                ),
                (
                    "chunk_bytes".into(),
                    FaultParameterValue::Integer(16 * 1024),
                ),
            ]),
            vec![
                integer_field(
                    "bytes_per_second",
                    "速率（B/s）",
                    "每秒最多发送的 Body 字节数。",
                    1,
                    100 * 1024 * 1024,
                ),
                integer_field(
                    "chunk_bytes",
                    "分块大小（字节）",
                    "每个发送分块的最大字节数。",
                    1,
                    1024 * 1024,
                ),
            ],
        ),
        "jitter_upstream" | "jitter_downstream" => (
            BTreeMap::from([
                (
                    "minimum_milliseconds".into(),
                    FaultParameterValue::Integer(0),
                ),
                (
                    "maximum_milliseconds".into(),
                    FaultParameterValue::Integer(100),
                ),
                ("per_chunk".into(), FaultParameterValue::Boolean(true)),
            ]),
            vec![
                integer_field(
                    "minimum_milliseconds",
                    "最小抖动（毫秒）",
                    "每次抖动的最短等待时间。",
                    0,
                    600_000,
                ),
                integer_field(
                    "maximum_milliseconds",
                    "最大抖动（毫秒）",
                    "每次抖动的最长等待时间。",
                    0,
                    600_000,
                ),
                boolean_field(
                    "per_chunk",
                    "每个分块均抖动",
                    "开启后每个分块独立抖动；关闭时仅消息发送前抖动一次。",
                ),
            ],
        ),
        "intermittent_upstream" | "intermittent_downstream" => (
            BTreeMap::from([
                (
                    "available_milliseconds".into(),
                    FaultParameterValue::Integer(1000),
                ),
                (
                    "blocked_milliseconds".into(),
                    FaultParameterValue::Integer(1000),
                ),
            ]),
            vec![
                integer_field(
                    "available_milliseconds",
                    "可用窗口（毫秒）",
                    "允许发送数据的连续时长。",
                    1,
                    600_000,
                ),
                integer_field(
                    "blocked_milliseconds",
                    "阻断窗口（毫秒）",
                    "暂停发送数据的连续时长。",
                    1,
                    600_000,
                ),
            ],
        ),
        "disconnect_upstream_mid_body" | "disconnect_downstream_mid_body" => one_integer(
            "after_bytes",
            "断连偏移（字节）",
            "成功发送前 N 字节后立即中止连接；必须小于实际 Body 长度。",
            1,
            0,
            i64::MAX,
        ),
        "reject_tls_handshake" | "disconnect_before_upstream" => (BTreeMap::new(), Vec::new()),
        _ => panic!("fault template {template_id} has no parameter definition"),
    }
}

pub(super) fn one_integer(
    key: &str,
    label: &str,
    description: &str,
    default: i64,
    minimum: i64,
    maximum: i64,
) -> (
    BTreeMap<String, FaultParameterValue>,
    Vec<FaultParameterFieldViewModel>,
) {
    (
        BTreeMap::from([(key.into(), FaultParameterValue::Integer(default))]),
        vec![integer_field(key, label, description, minimum, maximum)],
    )
}

pub(super) fn integer_field(
    key: &str,
    label: &str,
    description: &str,
    minimum: i64,
    maximum: i64,
) -> FaultParameterFieldViewModel {
    FaultParameterFieldViewModel {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: FaultParameterKind::Integer,
        required: true,
        minimum: Some(minimum),
        maximum: Some(maximum),
        multiline: false,
    }
}

pub(super) fn boolean_field(
    key: &str,
    label: &str,
    description: &str,
) -> FaultParameterFieldViewModel {
    FaultParameterFieldViewModel {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: FaultParameterKind::Boolean,
        required: true,
        minimum: None,
        maximum: None,
        multiline: false,
    }
}

pub(super) fn text_field(
    key: &str,
    label: &str,
    description: &str,
    multiline: bool,
) -> FaultParameterFieldViewModel {
    FaultParameterFieldViewModel {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        kind: FaultParameterKind::Text,
        required: true,
        minimum: None,
        maximum: None,
        multiline,
    }
}

pub(super) fn multiline_text_field(
    key: &str,
    label: &str,
    description: &str,
) -> FaultParameterFieldViewModel {
    text_field(key, label, description, true)
}

pub(super) fn json_field(
    key: &str,
    label: &str,
    description: &str,
) -> FaultParameterFieldViewModel {
    FaultParameterFieldViewModel {
        kind: FaultParameterKind::Json,
        ..text_field(key, label, description, true)
    }
}
