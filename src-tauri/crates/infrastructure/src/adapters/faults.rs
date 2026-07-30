use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use gmofg_proxy_application::{
    ActiveFaultViewModel, AppError, AppResult, FaultConfigurationDraft,
    FaultParameterFieldViewModel, FaultParameterKind, FaultParameterValue, FaultServicePort,
    FaultTemplateViewModel, MessageStage, RuleDraft, RuleRepositoryPort, UiTone,
};
use gmofg_proxy_domain::{
    DropResponseMode, JitterScope, MatchCondition, MatchField, MatchOperator, RuleAction,
    TerminalAction, TrafficDirection,
};
use gmofg_proxy_runtime::codec::encode_strict;
use serde_json::Value;

use super::rules::{RuleRepositoryAdapter, action_to_app, condition_to_app};

#[derive(Debug)]
pub struct FaultServiceAdapter {
    rules: Arc<RuleRepositoryAdapter>,
}

impl FaultServiceAdapter {
    #[must_use]
    pub fn new(rules: Arc<RuleRepositoryAdapter>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl FaultServicePort for FaultServiceAdapter {
    async fn templates(&self) -> AppResult<Vec<FaultTemplateViewModel>> {
        Ok(template_definitions()
            .into_iter()
            .map(|template| template.view)
            .collect())
    }

    async fn configure(
        &self,
        configuration: FaultConfigurationDraft,
    ) -> AppResult<ActiveFaultViewModel> {
        let definition = template_definitions()
            .into_iter()
            .find(|template| template.view.template_id == configuration.template_id)
            .ok_or_else(|| AppError::new("RULE_INVALID", "故障模板不存在。"))?;
        let (stage, action) = (definition.action)(&configuration.parameters)?;
        let conditions = configuration_conditions(&configuration, stage)?;
        let rule = self
            .rules
            .save(RuleDraft {
                rule_id: configuration.existing_rule_id,
                expected_revision: configuration.expected_revision,
                name: format!("故障模拟·{}", definition.view.name),
                description: format!("fault:{}", definition.view.template_id),
                enabled: true,
                priority: configuration.priority,
                channel: configuration.channel,
                stage: Some(stage),
                conditions: conditions.iter().map(condition_to_app).collect(),
                actions: vec![
                    action_to_app(&action)
                        .map_err(|error| AppError::new("RULE_INVALID", error.to_string()))?,
                ],
                one_shot: configuration.one_shot,
            })
            .await?;
        Ok(active_from_rule(&rule, &definition.view.name))
    }

    async fn active(&self) -> AppResult<Vec<ActiveFaultViewModel>> {
        let rules = self.rules.list().await?;
        Ok(rules
            .into_iter()
            .filter(|rule| rule.name.starts_with("故障模拟·"))
            .map(|rule| ActiveFaultViewModel {
                rule_id: rule.rule_id,
                template_name: rule.name.trim_start_matches("故障模拟·").into(),
                target_summary: rule.match_summary,
                priority: rule.priority,
                hit_count: rule.hit_count,
                enabled: rule.enabled,
                status_text: if rule.enabled {
                    "活动中".into()
                } else {
                    "已停用".into()
                },
                ui_tone: if rule.enabled {
                    UiTone::Warning
                } else {
                    UiTone::Neutral
                },
                revision: rule.revision,
            })
            .collect())
    }

    async fn stop(
        &self,
        rule_id: gmofg_proxy_application::RuleId,
        expected_revision: u64,
    ) -> AppResult<ActiveFaultViewModel> {
        let rule = self.rules.toggle(rule_id, expected_revision, false).await?;
        Ok(active_from_rule(
            &rule,
            rule.summary.name.trim_start_matches("故障模拟·"),
        ))
    }
}

fn configuration_conditions(
    configuration: &FaultConfigurationDraft,
    stage: MessageStage,
) -> AppResult<Vec<MatchCondition>> {
    if stage == MessageStage::TlsHandshake {
        let mut field_errors = BTreeMap::new();
        if configuration
            .terminal
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            field_errors.insert(
                "terminal".into(),
                vec!["TLS 握手阶段不能按终端 IP 匹配，请在规则页面使用客户端证书指纹。".into()],
            );
        }
        if configuration
            .target
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            field_errors.insert(
                "target".into(),
                vec!["TLS 握手阶段尚未解析 HTTP 路径，不能配置路径条件。".into()],
            );
        }
        if !field_errors.is_empty() {
            return Err(AppError::field(
                "RULE_INVALID",
                "TLS 握手故障包含不支持的匹配条件。",
                field_errors,
            ));
        }
        return Ok(configuration
            .nth_hit
            .map(|nth| vec![MatchCondition::NthHit(u64::from(nth))])
            .unwrap_or_default());
    }

    let mut conditions = Vec::new();
    if let Some(terminal) = configuration
        .terminal
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(MatchCondition::Field {
            field: MatchField::TerminalIp,
            operator: MatchOperator::Equals(terminal.clone()),
        });
    }
    if let Some(target) = configuration
        .target
        .as_ref()
        .filter(|value| !value.is_empty())
    {
        conditions.push(MatchCondition::Field {
            field: MatchField::PathOrRequestType,
            operator: MatchOperator::Contains(target.clone()),
        });
    }
    if let Some(nth) = configuration.nth_hit {
        conditions.push(MatchCondition::NthHit(u64::from(nth)));
    }
    Ok(conditions)
}

struct TemplateDefinition {
    view: FaultTemplateViewModel,
    action: TemplateAction,
}

type FaultParameters = BTreeMap<String, FaultParameterValue>;
type TemplateAction = fn(&FaultParameters) -> AppResult<(MessageStage, RuleAction)>;

#[allow(clippy::too_many_lines)]
fn template_definitions() -> Vec<TemplateDefinition> {
    vec![
        template(
            "reject_tls_handshake",
            "拒绝 TLS 握手",
            "TLS 握手阶段",
            "在 HTTP 消息进入规则管线前拒绝客户端握手",
            "Payment App",
            "高",
            reject_tls,
        ),
        template(
            "disconnect_before_upstream",
            "不连接上游并断开",
            "请求阶段",
            "不建立上游连接并关闭 App 连接",
            "Payment App",
            "高",
            disconnect,
        ),
        template(
            "request_delay",
            "请求前延迟/超时",
            "请求阶段",
            "转发前等待指定时间",
            "Payment App 与 Server",
            "中",
            request_delay,
        ),
        template(
            "modify_request_json",
            "修改请求 JSON",
            "请求阶段",
            "修改指定 JSON 字段",
            "GMO-FG Server",
            "中",
            modify_json,
        ),
        template(
            "drop_upstream_response",
            "发送上游后丢弃响应",
            "请求阶段",
            "读取响应后不返回 App 并断开",
            "Payment App",
            "高",
            drop_response,
        ),
        template(
            "upstream_connect_timeout",
            "上游连接超时",
            "请求阶段",
            "保持上游连接直至超时",
            "Payment App",
            "高",
            connect_timeout,
        ),
        template(
            "upstream_write_timeout",
            "上游写入超时",
            "请求阶段",
            "连接上游后在写入请求时保持至超时",
            "Payment App",
            "高",
            write_timeout,
        ),
        template(
            "upstream_read_timeout",
            "上游读取超时",
            "请求阶段",
            "写入请求后在读取上游响应时保持至超时",
            "Payment App",
            "高",
            read_timeout,
        ),
        template(
            "response_delay",
            "响应延迟",
            "响应阶段",
            "返回 App 前等待指定时间",
            "Payment App",
            "中",
            response_delay,
        ),
        template(
            "custom_http_status",
            "自定义 HTTP 状态",
            "响应阶段",
            "返回指定 HTTP 状态码",
            "Payment App",
            "中",
            custom_status,
        ),
        template(
            "mock_shift_jis_json",
            "Mock Shift-JIS JSON",
            "请求阶段",
            "绕过上游并返回 Mock",
            "Payment App",
            "高",
            mock_response,
        ),
        template(
            "invalid_json",
            "非法 JSON",
            "响应阶段",
            "返回可编码但语法非法的 JSON",
            "Payment App",
            "高",
            invalid_json,
        ),
        template(
            "wrong_content_length",
            "错误 Content-Length",
            "响应阶段",
            "声明长度与真实 Body 不一致",
            "Payment App",
            "高",
            wrong_length,
        ),
        template(
            "truncate_response",
            "截断响应",
            "响应阶段",
            "发送前 N 字节后断开",
            "Payment App",
            "高",
            truncate,
        ),
        template(
            "throttle_upstream",
            "上行限速",
            "请求阶段",
            "按指定速率分块发送请求 Body",
            "GMO-FG Server",
            "中",
            throttle_upstream,
        ),
        template(
            "throttle_downstream",
            "下行限速",
            "响应阶段",
            "按指定速率分块返回响应 Body",
            "Payment App",
            "中",
            throttle_downstream,
        ),
        template(
            "jitter_upstream",
            "上行抖动",
            "请求阶段",
            "请求 Body 每个分块发送前加入确定性随机抖动",
            "GMO-FG Server",
            "中",
            jitter_upstream,
        ),
        template(
            "jitter_downstream",
            "下行抖动",
            "响应阶段",
            "响应 Body 每个分块发送前加入确定性随机抖动",
            "Payment App",
            "中",
            jitter_downstream,
        ),
        template(
            "intermittent_upstream",
            "上行间歇通断",
            "请求阶段",
            "按可用窗口和阻断窗口循环发送请求 Body",
            "GMO-FG Server",
            "高",
            intermittent_upstream,
        ),
        template(
            "intermittent_downstream",
            "下行间歇通断",
            "响应阶段",
            "按可用窗口和阻断窗口循环返回响应 Body",
            "Payment App",
            "高",
            intermittent_downstream,
        ),
        template(
            "disconnect_upstream_mid_body",
            "上行 Body 中途断连",
            "请求阶段",
            "发送指定字节数后中止上游请求",
            "GMO-FG Server",
            "高",
            disconnect_upstream_mid_body,
        ),
        template(
            "disconnect_downstream_mid_body",
            "下行 Body 中途断连",
            "响应阶段",
            "返回指定字节数后中止 App 响应",
            "Payment App",
            "高",
            disconnect_downstream_mid_body,
        ),
    ]
}

#[allow(clippy::unnecessary_wraps)]
fn reject_tls(_: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::TlsHandshake,
        RuleAction::Terminal(TerminalAction::RejectTlsHandshake),
    ))
}

fn template(
    id: &str,
    name: &str,
    stage: &str,
    behavior: &str,
    affected: &str,
    risk: &str,
    action: TemplateAction,
) -> TemplateDefinition {
    let (default_parameters, parameter_schema) = parameter_definitions(id);
    TemplateDefinition {
        view: FaultTemplateViewModel {
            template_id: id.into(),
            name: name.into(),
            stage_text: stage.into(),
            behavior_text: behavior.into(),
            affected_party_text: affected.into(),
            default_channel: gmofg_proxy_application::ChannelKind::Transaction,
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
        action,
    }
}

#[allow(clippy::too_many_lines)]
fn parameter_definitions(
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
            "返回给 Payment App 的 HTTP 状态码。",
            500,
            100,
            599,
        ),
        "mock_shift_jis_json" => (
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
                    "Shift-JIS JSON Body",
                    "必须是合法 JSON，且所有字符必须可严格编码为 Shift-JIS。",
                ),
            ],
        ),
        "invalid_json" => (
            BTreeMap::from([("body".into(), FaultParameterValue::Text("{invalid".into()))]),
            vec![multiline_text_field(
                "body",
                "非法 JSON Body",
                "必须保持 JSON 语法非法，且所有字符必须可严格编码为 Shift-JIS。",
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

fn one_integer(
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

fn integer_field(
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

fn boolean_field(key: &str, label: &str, description: &str) -> FaultParameterFieldViewModel {
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

fn text_field(
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

fn multiline_text_field(key: &str, label: &str, description: &str) -> FaultParameterFieldViewModel {
    text_field(key, label, description, true)
}

fn json_field(key: &str, label: &str, description: &str) -> FaultParameterFieldViewModel {
    FaultParameterFieldViewModel {
        kind: FaultParameterKind::Json,
        ..text_field(key, label, description, true)
    }
}

#[allow(clippy::unnecessary_wraps)]
fn disconnect(_: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::DisconnectBeforeUpstream),
    ))
}

fn request_delay(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((MessageStage::Request, delay(values)?))
}

fn response_delay(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((MessageStage::Response, delay(values)?))
}

fn delay(values: &FaultParameters) -> AppResult<RuleAction> {
    Ok(RuleAction::Delay {
        milliseconds: u64_parameter(values, "milliseconds")?,
    })
}

fn modify_json(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let value_text = json_parameter(values, "value")?;
    let value = serde_json::from_str(value_text).map_err(|error| {
        parameter_error("value", format!("参数 value 必须包含合法 JSON：{error}"))
    })?;
    Ok((
        MessageStage::Request,
        RuleAction::SetJsonField {
            path: text_parameter(values, "path")?.to_owned(),
            value,
        },
    ))
}

fn drop_response(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let mode = if boolean_parameter(values, "close_after_request_write")? {
        DropResponseMode::CloseAfterRequestWrite
    } else {
        DropResponseMode::ReadCompleteResponse
    };
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::DropUpstreamResponse { mode }),
    ))
}

fn connect_timeout(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::UpstreamConnectTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

fn write_timeout(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::UpstreamWriteTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

fn read_timeout(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::UpstreamReadTimeout {
            milliseconds: u64_parameter(values, "milliseconds")?,
        }),
    ))
}

fn custom_status(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let status = status_parameter(values)?;
    Ok((
        MessageStage::Response,
        RuleAction::CustomHttpStatus { status },
    ))
}

fn mock_response(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let status = status_parameter(values)?;
    let body = json_parameter(values, "body")?;
    serde_json::from_str::<Value>(body)
        .map_err(|error| parameter_error("body", format!("Mock Body 不是合法 JSON：{error}")))?;
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::MockResponse {
            status,
            headers: Vec::new(),
            shift_jis_body: strict_shift_jis(body)?,
        }),
    ))
}

fn invalid_json(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let body = text_parameter(values, "body")?;
    if serde_json::from_str::<Value>(body).is_ok() {
        return Err(parameter_error(
            "body",
            "非法 JSON 模板的 Body 必须保持语法非法。",
        ));
    }
    Ok((
        MessageStage::Response,
        RuleAction::Terminal(TerminalAction::InvalidJson {
            shift_jis_body: strict_shift_jis(body)?,
        }),
    ))
}

fn wrong_length(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    let delta = integer_parameter(values, "delta")?;
    Ok((
        MessageStage::Response,
        RuleAction::Terminal(TerminalAction::IncorrectContentLength { delta }),
    ))
}

fn truncate(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Response,
        RuleAction::Terminal(TerminalAction::TruncateResponse {
            bytes: u64_parameter(values, "bytes")?,
        }),
    ))
}

fn throttle_upstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    throttle(values, MessageStage::Request, TrafficDirection::Upstream)
}

fn throttle_downstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    throttle(values, MessageStage::Response, TrafficDirection::Downstream)
}

fn throttle(
    values: &FaultParameters,
    stage: MessageStage,
    direction: TrafficDirection,
) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        stage,
        RuleAction::Throttle {
            bytes_per_second: u64_parameter(values, "bytes_per_second")?,
            chunk_bytes: u64_parameter(values, "chunk_bytes")?,
            direction,
        },
    ))
}

fn jitter_upstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    jitter(values, MessageStage::Request)
}

fn jitter_downstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    jitter(values, MessageStage::Response)
}

fn jitter(values: &FaultParameters, stage: MessageStage) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        stage,
        RuleAction::Jitter {
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

fn intermittent_upstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    intermittent(values, MessageStage::Request, TrafficDirection::Upstream)
}

fn intermittent_downstream(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    intermittent(values, MessageStage::Response, TrafficDirection::Downstream)
}

fn intermittent(
    values: &FaultParameters,
    stage: MessageStage,
    direction: TrafficDirection,
) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        stage,
        RuleAction::Intermittent {
            available_milliseconds: u64_parameter(values, "available_milliseconds")?,
            blocked_milliseconds: u64_parameter(values, "blocked_milliseconds")?,
            direction,
        },
    ))
}

fn disconnect_upstream_mid_body(values: &FaultParameters) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Request,
        RuleAction::Terminal(TerminalAction::DisconnectDuringUpstreamWrite {
            after_bytes: u64_parameter(values, "after_bytes")?,
        }),
    ))
}

fn disconnect_downstream_mid_body(
    values: &FaultParameters,
) -> AppResult<(MessageStage, RuleAction)> {
    Ok((
        MessageStage::Response,
        RuleAction::Terminal(TerminalAction::DisconnectDuringDownstreamWrite {
            after_bytes: u64_parameter(values, "after_bytes")?,
        }),
    ))
}

fn status_parameter(values: &FaultParameters) -> AppResult<u16> {
    let status = integer_parameter(values, "status")?;
    if !(100..=599).contains(&status) {
        return Err(parameter_error(
            "status",
            "参数 status 必须是 100 到 599 之间的整数。",
        ));
    }
    u16::try_from(status).map_err(|_| parameter_error("status", "HTTP 状态码超出范围。"))
}

fn u64_parameter(values: &FaultParameters, name: &str) -> AppResult<u64> {
    let value = integer_parameter(values, name)?;
    u64::try_from(value).map_err(|_| parameter_error(name, format!("参数 {name} 必须是非负整数。")))
}

fn integer_parameter(values: &FaultParameters, name: &str) -> AppResult<i64> {
    match values.get(name) {
        Some(FaultParameterValue::Integer(value)) => Ok(*value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是整数。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

fn boolean_parameter(values: &FaultParameters, name: &str) -> AppResult<bool> {
    match values.get(name) {
        Some(FaultParameterValue::Boolean(value)) => Ok(*value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是布尔值。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

fn text_parameter<'a>(values: &'a FaultParameters, name: &str) -> AppResult<&'a str> {
    match values.get(name) {
        Some(FaultParameterValue::Text(value)) => Ok(value),
        Some(_) => Err(parameter_error(name, format!("参数 {name} 必须是文本。"))),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

fn json_parameter<'a>(values: &'a FaultParameters, name: &str) -> AppResult<&'a str> {
    match values.get(name) {
        Some(FaultParameterValue::Json(value)) => Ok(value),
        Some(_) => Err(parameter_error(
            name,
            format!("参数 {name} 必须是 JSON 文本。"),
        )),
        None => Err(parameter_error(name, format!("缺少必填参数 {name}。"))),
    }
}

fn parameter_error(name: &str, message: impl Into<String>) -> AppError {
    AppError::field(
        "RULE_INVALID",
        "故障参数无效。",
        BTreeMap::from([(format!("parameters.{name}"), vec![message.into()])]),
    )
}

fn strict_shift_jis(text: &str) -> AppResult<Vec<u8>> {
    encode_strict(text).map_err(|error| AppError::new(error.code, error.message))
}

fn active_from_rule(
    rule: &gmofg_proxy_application::RuleViewModel,
    template_name: &str,
) -> ActiveFaultViewModel {
    ActiveFaultViewModel {
        rule_id: rule.summary.rule_id,
        template_name: template_name.into(),
        target_summary: rule.summary.match_summary.clone(),
        priority: rule.summary.priority,
        hit_count: rule.summary.hit_count,
        enabled: rule.summary.enabled,
        status_text: if rule.summary.enabled {
            "活动中".into()
        } else {
            "已停用".into()
        },
        ui_tone: if rule.summary.enabled {
            UiTone::Warning
        } else {
            UiTone::Neutral
        },
        revision: rule.summary.revision,
    }
}

#[cfg(test)]
mod tests {
    use gmofg_proxy_runtime::codec::decode_strict;

    use super::*;

    #[test]
    fn required_terminal_faults_use_domain_compatible_stages() {
        let definitions = template_definitions();
        let ids = definitions
            .iter()
            .map(|definition| definition.view.template_id.as_str())
            .collect::<Vec<_>>();
        for required in [
            "reject_tls_handshake",
            "drop_upstream_response",
            "upstream_connect_timeout",
            "upstream_write_timeout",
            "upstream_read_timeout",
            "throttle_upstream",
            "throttle_downstream",
            "jitter_upstream",
            "jitter_downstream",
            "intermittent_upstream",
            "intermittent_downstream",
            "disconnect_upstream_mid_body",
            "disconnect_downstream_mid_body",
        ] {
            assert!(ids.contains(&required), "missing template {required}");
        }
        assert_eq!(
            reject_tls(&BTreeMap::new()).expect("tls").0,
            MessageStage::TlsHandshake
        );
        assert_eq!(
            drop_response(&BTreeMap::from([(
                "close_after_request_write".into(),
                FaultParameterValue::Boolean(false),
            )]))
            .expect("drop")
            .0,
            MessageStage::Request
        );
        assert_eq!(
            write_timeout(&BTreeMap::from([(
                "milliseconds".into(),
                FaultParameterValue::Integer(70_000),
            )]))
            .expect("write")
            .0,
            MessageStage::Request
        );
        assert_eq!(
            read_timeout(&BTreeMap::from([(
                "milliseconds".into(),
                FaultParameterValue::Integer(70_000),
            )]))
            .expect("read")
            .0,
            MessageStage::Request
        );
    }

    #[test]
    fn mock_and_invalid_json_use_strict_shift_jis() {
        let mock_parameters = BTreeMap::from([
            ("status".into(), FaultParameterValue::Integer(200)),
            (
                "body".into(),
                FaultParameterValue::Json("{\"結果\":\"成功\"}".into()),
            ),
        ]);
        let (_, mock) = mock_response(&mock_parameters).expect("mock");
        let RuleAction::Terminal(TerminalAction::MockResponse { shift_jis_body, .. }) = mock else {
            panic!("mock response action");
        };
        assert_eq!(
            decode_strict(&shift_jis_body).expect("decode"),
            "{\"結果\":\"成功\"}"
        );

        let invalid_parameters = BTreeMap::from([(
            "body".into(),
            FaultParameterValue::Text("{\"結果\":".into()),
        )]);
        let (_, invalid) = invalid_json(&invalid_parameters).expect("invalid");
        let RuleAction::Terminal(TerminalAction::InvalidJson { shift_jis_body }) = invalid else {
            panic!("invalid json action");
        };
        assert_eq!(
            decode_strict(&shift_jis_body).expect("decode"),
            "{\"結果\":"
        );

        let unencodable = BTreeMap::from([
            ("status".into(), FaultParameterValue::Integer(200)),
            (
                "body".into(),
                FaultParameterValue::Json("{\"value\":\"🧪\"}".into()),
            ),
        ]);
        assert_eq!(
            mock_response(&unencodable)
                .expect_err("strict encoding")
                .view_model
                .code,
            "SHIFT_JIS_ENCODE_FAILED"
        );
        assert_eq!(
            invalid_json(&BTreeMap::from([(
                "body".into(),
                FaultParameterValue::Text("🧪{".into()),
            )]))
            .expect_err("strict encoding")
            .view_model
            .code,
            "SHIFT_JIS_ENCODE_FAILED"
        );
        assert_eq!(
            invalid_json(&BTreeMap::from([(
                "body".into(),
                FaultParameterValue::Text("{}".into()),
            )]))
            .expect_err("must remain invalid")
            .view_model
            .code,
            "RULE_INVALID"
        );
    }

    #[test]
    fn every_template_exposes_matching_typed_defaults_and_schema() {
        for definition in template_definitions() {
            assert_eq!(
                definition.view.default_channel,
                gmofg_proxy_application::ChannelKind::Transaction
            );
            assert_eq!(definition.view.default_nth_hit, 1);
            assert!(!definition.view.default_one_shot);
            assert_eq!(definition.view.default_priority, 100);
            assert_eq!(
                definition.view.default_parameters.len(),
                definition.view.parameter_schema.len(),
                "{}",
                definition.view.template_id
            );
            for field in &definition.view.parameter_schema {
                let value = definition
                    .view
                    .default_parameters
                    .get(&field.key)
                    .unwrap_or_else(|| {
                        panic!(
                            "{} is missing default for {}",
                            definition.view.template_id, field.key
                        )
                    });
                assert!(
                    matches!(
                        (&field.kind, value),
                        (FaultParameterKind::Boolean, FaultParameterValue::Boolean(_))
                            | (FaultParameterKind::Integer, FaultParameterValue::Integer(_))
                            | (FaultParameterKind::Text, FaultParameterValue::Text(_))
                            | (FaultParameterKind::Json, FaultParameterValue::Json(_))
                    ),
                    "{} has mismatched default for {}",
                    definition.view.template_id,
                    field.key
                );
            }
        }
    }

    // FAULT-001~007, ACTION-001~013, TEST-FAULT:
    // every visible template default must compile into the shared domain rule engine.
    #[test]
    fn every_template_default_produces_a_domain_valid_action_for_its_declared_stage() {
        for definition in template_definitions() {
            let (stage, action) = (definition.action)(&definition.view.default_parameters)
                .unwrap_or_else(|error| {
                    panic!(
                        "{} default parameters failed: {error}",
                        definition.view.template_id
                    )
                });
            let domain_stage = match stage {
                MessageStage::TlsHandshake => gmofg_proxy_domain::MessageStage::TlsHandshake,
                MessageStage::Request => gmofg_proxy_domain::MessageStage::Request,
                MessageStage::Response => gmofg_proxy_domain::MessageStage::Response,
                MessageStage::Terminal => {
                    panic!(
                        "{} default unexpectedly targets a terminal event",
                        definition.view.template_id
                    )
                }
            };
            let conditions = vec![gmofg_proxy_domain::MatchCondition::NthHit(u64::from(
                definition.view.default_nth_hit,
            ))];
            let draft = gmofg_proxy_domain::RuleDraft {
                expected_revision: None,
                name: definition.view.name.clone(),
                description: definition.view.behavior_text.clone(),
                enabled: true,
                priority: u32::try_from(definition.view.default_priority)
                    .expect("non-negative default priority"),
                created_order: 1,
                channel: Some(gmofg_proxy_domain::ChannelKind::Transaction),
                stage: domain_stage,
                conditions,
                actions: vec![action],
                one_shot: definition.view.default_one_shot,
            };
            gmofg_proxy_domain::validate_rule_draft(&draft).unwrap_or_else(|error| {
                panic!(
                    "{} default does not produce a valid domain rule: {error}",
                    definition.view.template_id
                )
            });
        }
    }

    // ACTION-001, FAULT-005~006, TEST-FAULT:
    // TLS faults preserve the same per-terminal Nth-hit contract as HTTP rules.
    #[test]
    fn tls_template_preserves_nth_hit_and_rejects_http_only_filters() {
        let defaults = FaultConfigurationDraft {
            template_id: "reject_tls_handshake".into(),
            existing_rule_id: None,
            expected_revision: None,
            channel: Some(gmofg_proxy_application::ChannelKind::Dll),
            terminal: None,
            target: None,
            nth_hit: Some(1),
            one_shot: false,
            priority: 100,
            parameters: BTreeMap::new(),
        };
        assert_eq!(
            configuration_conditions(&defaults, MessageStage::TlsHandshake)
                .expect("default TLS configuration"),
            vec![gmofg_proxy_domain::MatchCondition::NthHit(1)]
        );

        let invalid = FaultConfigurationDraft {
            terminal: Some("10.0.34.94".into()),
            target: Some("/".into()),
            ..defaults
        };
        let error = configuration_conditions(&invalid, MessageStage::TlsHandshake)
            .expect_err("HTTP-only TLS filters");
        for field in ["terminal", "target"] {
            assert!(
                error.view_model.field_errors.contains_key(field),
                "missing field error for {field}"
            );
        }
    }

    #[test]
    fn wrong_boolean_number_and_body_types_return_stable_field_errors() {
        let cases = [
            (
                drop_response(&BTreeMap::from([(
                    "close_after_request_write".into(),
                    FaultParameterValue::Text("false".into()),
                )])),
                "parameters.close_after_request_write",
            ),
            (
                request_delay(&BTreeMap::from([(
                    "milliseconds".into(),
                    FaultParameterValue::Text("70000".into()),
                )])),
                "parameters.milliseconds",
            ),
            (
                mock_response(&BTreeMap::from([
                    ("status".into(), FaultParameterValue::Integer(200)),
                    ("body".into(), FaultParameterValue::Boolean(false)),
                ])),
                "parameters.body",
            ),
        ];

        for (result, expected_field) in cases {
            let error = result.expect_err("wrong parameter type must fail");
            assert_eq!(error.view_model.code, "RULE_INVALID");
            assert_eq!(error.view_model.message, "故障参数无效。");
            assert_eq!(
                error
                    .view_model
                    .field_errors
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                vec![expected_field]
            );
        }
    }

    #[test]
    fn missing_required_parameter_does_not_use_a_fallback() {
        let error = request_delay(&BTreeMap::new()).expect_err("missing milliseconds");
        assert_eq!(error.view_model.code, "RULE_INVALID");
        assert_eq!(
            error.view_model.field_errors["parameters.milliseconds"],
            vec!["缺少必填参数 milliseconds。"]
        );
    }
}
