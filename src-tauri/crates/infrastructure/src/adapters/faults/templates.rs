use super::{
    AppError, AppResult, BTreeMap, BodyCodec, FaultParameterValue, FaultTemplateViewModel,
    MessageStage, ProductFaultTemplate, RuleAction, UiTone, connect_timeout, custom_status,
    disconnect, disconnect_downstream_mid_body, disconnect_upstream_mid_body, drop_response,
    encoded_template, intermittent_downstream, intermittent_upstream, invalid_json,
    jitter_downstream, jitter_upstream, mock_response, modify_json, read_timeout, reject_tls,
    request_delay, response_delay, template, throttle_downstream, throttle_upstream, truncate,
    write_timeout, wrong_length,
};

pub(super) struct TemplateDefinition {
    pub(super) view: FaultTemplateViewModel,
    pub(super) action: TemplateAction,
}

pub(super) type FaultParameters = BTreeMap<String, FaultParameterValue>;
pub(super) enum TemplateAction {
    Plain(fn(&FaultParameters) -> AppResult<(MessageStage, RuleAction)>),
    Encoded(fn(&FaultParameters, &dyn BodyCodec) -> AppResult<(MessageStage, RuleAction)>),
}

impl TemplateAction {
    pub(super) fn invoke(
        &self,
        parameters: &FaultParameters,
        body_codec: &dyn BodyCodec,
    ) -> AppResult<(MessageStage, RuleAction)> {
        match self {
            Self::Plain(action) => action(parameters),
            Self::Encoded(action) => action(parameters, body_codec),
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(super) fn generic_template_definitions() -> Vec<TemplateDefinition> {
    vec![
        template(
            "reject_tls_handshake",
            "拒绝 TLS 握手",
            "TLS 握手阶段",
            "在 HTTP 消息进入规则管线前拒绝客户端握手",
            "客户端",
            "高",
            reject_tls,
        ),
        template(
            "disconnect_before_upstream",
            "不连接上游并断开",
            "请求阶段",
            "不建立上游连接并关闭 App 连接",
            "客户端",
            "高",
            disconnect,
        ),
        template(
            "request_delay",
            "请求前延迟/超时",
            "请求阶段",
            "转发前等待指定时间",
            "客户端与上游服务",
            "中",
            request_delay,
        ),
        template(
            "modify_request_json",
            "修改请求 JSON",
            "请求阶段",
            "修改指定 JSON 字段",
            "上游服务",
            "中",
            modify_json,
        ),
        template(
            "drop_upstream_response",
            "发送上游后丢弃响应",
            "请求阶段",
            "读取响应后不返回 App 并断开",
            "客户端",
            "高",
            drop_response,
        ),
        template(
            "upstream_connect_timeout",
            "上游连接超时",
            "请求阶段",
            "保持上游连接直至超时",
            "客户端",
            "高",
            connect_timeout,
        ),
        template(
            "upstream_write_timeout",
            "上游写入超时",
            "请求阶段",
            "连接上游后在写入请求时保持至超时",
            "客户端",
            "高",
            write_timeout,
        ),
        template(
            "upstream_read_timeout",
            "上游读取超时",
            "请求阶段",
            "写入请求后在读取上游响应时保持至超时",
            "客户端",
            "高",
            read_timeout,
        ),
        template(
            "response_delay",
            "响应延迟",
            "响应阶段",
            "返回 App 前等待指定时间",
            "客户端",
            "中",
            response_delay,
        ),
        template(
            "custom_http_status",
            "自定义 HTTP 状态",
            "响应阶段",
            "返回指定 HTTP 状态码",
            "客户端",
            "中",
            custom_status,
        ),
        encoded_template(
            "mock_json",
            "Mock JSON",
            "请求阶段",
            "绕过上游并返回 Mock",
            "客户端",
            "高",
            mock_response,
        ),
        encoded_template(
            "invalid_json",
            "非法 JSON",
            "响应阶段",
            "返回可编码但语法非法的 JSON",
            "客户端",
            "高",
            invalid_json,
        ),
        template(
            "wrong_content_length",
            "错误 Content-Length",
            "响应阶段",
            "声明长度与真实 Body 不一致",
            "客户端",
            "高",
            wrong_length,
        ),
        template(
            "truncate_response",
            "截断响应",
            "响应阶段",
            "发送前 N 字节后断开",
            "客户端",
            "高",
            truncate,
        ),
        template(
            "throttle_upstream",
            "上行限速",
            "请求阶段",
            "按指定速率分块发送请求 Body",
            "上游服务",
            "中",
            throttle_upstream,
        ),
        template(
            "throttle_downstream",
            "下行限速",
            "响应阶段",
            "按指定速率分块返回响应 Body",
            "客户端",
            "中",
            throttle_downstream,
        ),
        template(
            "jitter_upstream",
            "上行抖动",
            "请求阶段",
            "请求 Body 每个分块发送前加入确定性随机抖动",
            "上游服务",
            "中",
            jitter_upstream,
        ),
        template(
            "jitter_downstream",
            "下行抖动",
            "响应阶段",
            "响应 Body 每个分块发送前加入确定性随机抖动",
            "客户端",
            "中",
            jitter_downstream,
        ),
        template(
            "intermittent_upstream",
            "上行间歇通断",
            "请求阶段",
            "按可用窗口和阻断窗口循环发送请求 Body",
            "上游服务",
            "高",
            intermittent_upstream,
        ),
        template(
            "intermittent_downstream",
            "下行间歇通断",
            "响应阶段",
            "按可用窗口和阻断窗口循环返回响应 Body",
            "客户端",
            "高",
            intermittent_downstream,
        ),
        template(
            "disconnect_upstream_mid_body",
            "上行 Body 中途断连",
            "请求阶段",
            "发送指定字节数后中止上游请求",
            "上游服务",
            "高",
            disconnect_upstream_mid_body,
        ),
        template(
            "disconnect_downstream_mid_body",
            "下行 Body 中途断连",
            "响应阶段",
            "返回指定字节数后中止 App 响应",
            "客户端",
            "高",
            disconnect_downstream_mid_body,
        ),
    ]
}

pub(super) fn template_definitions(
    catalog: &[ProductFaultTemplate],
) -> AppResult<Vec<TemplateDefinition>> {
    let definitions = generic_template_definitions();
    // Intercept Proxy 没有静态产品通道，也不携带任何业务模板。空产品目录在这里表示
    // “使用全部通用网络故障”，而不是让故障页面变成空白。Application 随后会把
    // `default` 占位通道替换成当前 Workspace 中真实的 Listener ID。
    if catalog.is_empty() {
        return Ok(definitions);
    }

    let mut generic = definitions
        .into_iter()
        .map(|definition| (definition.view.template_id.clone(), definition))
        .collect::<BTreeMap<_, _>>();
    catalog
        .iter()
        .map(|metadata| {
            let mut definition = generic.remove(metadata.id).ok_or_else(|| {
                AppError::new(
                    "PRODUCT_PROFILE_INVALID",
                    format!("产品声明了未知故障能力：{}", metadata.id),
                )
            })?;
            definition.view.name = metadata.name.into();
            definition.view.stage_text = metadata.stage_text.into();
            definition.view.behavior_text = metadata.behavior_text.into();
            definition.view.affected_party_text = metadata.affected_party_text.into();
            definition.view.default_channel =
                intercept_proxy_domain::ChannelId::new(metadata.default_channel_id)
                    .map_err(AppError::from)?;
            definition.view.risk_text = metadata.risk_text.into();
            definition.view.ui_tone = if metadata.risk_text == "高" {
                UiTone::Danger
            } else {
                UiTone::Warning
            };
            Ok(definition)
        })
        .collect()
}
