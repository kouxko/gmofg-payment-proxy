//! 断点编辑内容的校验与规范化。
//!
//! 前端只提交输入；本模块校验状态码、Header、JSON，调用产品编码器重建字节并同步
//! `Content-Length`，从而让桌面 UI 与未来 TUI/CLI 共用同一套报文修改规则。

use std::{collections::BTreeMap, sync::Arc};

use http::{HeaderName, HeaderValue};
use intercept_proxy_product_api::BodyCodec;

use crate::{
    AppError, AppResult, BreakpointDecision, BreakpointDecisionKind, BreakpointDetailViewModel,
    BreakpointDraft, BreakpointValidationPort, BreakpointValidationViewModel, MessageContentKind,
    MessageContentViewModel, MessageStage,
};

/// 断点编辑在 Rust 侧的唯一校验器和规范化器。
#[derive(Debug)]
pub struct BreakpointValidator {
    body_codec_resolver: Arc<dyn BreakpointBodyCodecResolver>,
}

pub trait BreakpointBodyCodecResolver: std::fmt::Debug + Send + Sync {
    fn resolve(&self, message: &MessageContentViewModel) -> Arc<dyn BodyCodec>;
}

#[derive(Debug)]
struct FixedBreakpointBodyCodecResolver {
    body_codec: Arc<dyn BodyCodec>,
}

impl BreakpointBodyCodecResolver for FixedBreakpointBodyCodecResolver {
    fn resolve(&self, _message: &MessageContentViewModel) -> Arc<dyn BodyCodec> {
        Arc::clone(&self.body_codec)
    }
}

impl BreakpointValidator {
    #[must_use]
    pub fn new(body_codec: Arc<dyn BodyCodec>) -> Self {
        Self::new_with_resolver(Arc::new(FixedBreakpointBodyCodecResolver { body_codec }))
    }

    #[must_use]
    pub fn new_with_resolver(body_codec_resolver: Arc<dyn BreakpointBodyCodecResolver>) -> Self {
        Self {
            body_codec_resolver,
        }
    }

    fn normalize_message(
        &self,
        mut message: MessageContentViewModel,
    ) -> AppResult<MessageContentViewModel> {
        if message
            .http_status
            .is_some_and(|status| !valid_http_status(status))
        {
            return Err(AppError::field(
                "HTTP_STATUS_INVALID",
                "HTTP 状态码无效。",
                BTreeMap::from([(
                    "message.http_status".into(),
                    vec!["HTTP 状态码必须位于 100 到 599。".into()],
                )]),
            ));
        }
        validate_headers(&message.headers)?;
        let body_codec = self.body_codec_resolver.resolve(&message);
        if let Some(text) = message.body_text.as_deref() {
            message.body_bytes = body_codec.encode(text).map_err(|error| {
                AppError::field(
                    error.code,
                    "报文正文无法使用当前产品编码器进行无损编码。",
                    BTreeMap::from([("message.body_text".into(), vec![error.message])]),
                )
            })?;
        }
        message.content_length = message.body_bytes.len();
        set_content_length(&mut message.headers, message.content_length);
        message.json = if message.content_kind == MessageContentKind::Json {
            message
                .body_text
                .as_deref()
                .filter(|text| !text.trim().is_empty())
                .map(serde_json::from_str)
                .transpose()
                .map_err(|error| {
                    AppError::field(
                        "JSON_INVALID",
                        "报文 JSON 无效。",
                        BTreeMap::from([("message.body_text".into(), vec![error.to_string()])]),
                    )
                })?
        } else {
            None
        };
        message.decode_error = None;
        Ok(message)
    }

    fn validate_message(&self, message: &MessageContentViewModel) -> BreakpointValidationViewModel {
        match self.normalize_message(message.clone()) {
            Ok(normalized)
                if normalized.body_bytes == message.body_bytes
                    && normalized.content_length == message.content_length
                    && normalized.headers == message.headers =>
            {
                valid()
            }
            Ok(_) => invalid(
                "message",
                "报文尚未规范化，请先执行 JSON 格式化或重新校验。",
            ),
            Err(error) => BreakpointValidationViewModel {
                valid: false,
                field_errors: error.view_model.field_errors,
                warnings: Vec::new(),
            },
        }
    }
}

impl BreakpointValidationPort for BreakpointValidator {
    fn format_json(&self, mut draft: BreakpointDraft) -> AppResult<BreakpointDraft> {
        if draft.message.content_kind != MessageContentKind::Json {
            return Err(AppError::new(
                "JSON_MEDIA_TYPE_REQUIRED",
                "只有 JSON Content-Type 的报文可以执行 JSON 格式化。",
            ));
        }
        let text =
            draft.message.body_text.as_deref().ok_or_else(|| {
                AppError::new("JSON_INVALID", "当前报文没有可格式化的文本 Body。")
            })?;
        let json: serde_json::Value = serde_json::from_str(text).map_err(|error| {
            AppError::field(
                "JSON_INVALID",
                "报文 JSON 无效。",
                BTreeMap::from([("message.body_text".into(), vec![error.to_string()])]),
            )
        })?;
        draft.message.body_text = Some(
            serde_json::to_string_pretty(&json)
                .map_err(|error| AppError::new("JSON_INVALID", error.to_string()))?,
        );
        draft.message.json = Some(json);
        draft.message = self.normalize_message(draft.message)?;
        Ok(draft)
    }

    fn restore_original(&self, detail: &BreakpointDetailViewModel) -> AppResult<BreakpointDraft> {
        Ok(BreakpointDraft {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: detail.summary.revision,
            message: detail.original.clone(),
        })
    }

    fn validate(
        &self,
        detail: &BreakpointDetailViewModel,
        draft: &BreakpointDraft,
    ) -> AppResult<BreakpointValidationViewModel> {
        if draft.breakpoint_id != detail.summary.breakpoint_id {
            return Ok(invalid("breakpoint_id", "断点标识与当前详情不一致。"));
        }
        if draft.expected_revision != detail.summary.revision {
            return Ok(invalid(
                "expected_revision",
                "断点已更新，请重新加载后再编辑。",
            ));
        }
        let mut validation = self.validate_message(&draft.message);
        if draft.message.start_line_bytes != detail.effective.start_line_bytes {
            push_error(
                &mut validation.field_errors,
                "message.start_line_bytes",
                "HTTP start-line 由 Rust 管理，不能通过断点编辑。",
            );
            validation.valid = false;
        }
        Ok(validation)
    }

    fn validate_decision(
        &self,
        detail: &BreakpointDetailViewModel,
        decision: &BreakpointDecision,
    ) -> AppResult<BreakpointValidationViewModel> {
        let mut validation = validate_breakpoint_decision_structure(detail, decision);
        if matches!(
            decision.kind,
            BreakpointDecisionKind::ForwardModified | BreakpointDecisionKind::MockResponse
        ) && let Some(message) = decision.message.as_ref()
        {
            merge_errors(
                &mut validation.field_errors,
                self.validate_message(message).field_errors,
            );
            validation.valid = validation.field_errors.is_empty();
        }
        if decision.kind == BreakpointDecisionKind::ForwardModified
            && decision.message.as_ref().is_some_and(|message| {
                message.start_line_bytes != detail.effective.start_line_bytes
            })
        {
            push_error(
                &mut validation.field_errors,
                "message.start_line_bytes",
                "HTTP start-line 由 Rust 管理，不能通过断点编辑。",
            );
            validation.valid = false;
        }
        Ok(validation)
    }
}

pub(crate) fn validate_breakpoint_decision_structure(
    detail: &BreakpointDetailViewModel,
    decision: &BreakpointDecision,
) -> BreakpointValidationViewModel {
    let mut errors = BTreeMap::<String, Vec<String>>::new();
    if decision.breakpoint_id != detail.summary.breakpoint_id {
        push_error(&mut errors, "breakpoint_id", "断点标识与当前详情不一致。");
    }
    if decision.expected_revision != detail.summary.revision {
        push_error(
            &mut errors,
            "expected_revision",
            "断点已更新，请重新加载后再处理。",
        );
    }
    if !stage_supports_decision(detail.summary.stage, decision.kind) {
        push_error(&mut errors, "kind", "该操作不适用于当前报文阶段。");
    }
    match decision.kind {
        BreakpointDecisionKind::ForwardModified | BreakpointDecisionKind::MockResponse => {
            if decision.message.is_none() {
                push_error(&mut errors, "message", "该操作必须提供报文。");
            }
        }
        BreakpointDecisionKind::Delay => {
            if decision
                .delay_ms
                .is_none_or(|value| value == 0 || value > 600_000)
            {
                push_error(
                    &mut errors,
                    "delay_ms",
                    "延迟时间必须位于 1 到 600000 毫秒。",
                );
            }
        }
        BreakpointDecisionKind::CustomHttpStatus => {
            if decision
                .http_status
                .is_none_or(|value| !valid_http_status(value))
            {
                push_error(
                    &mut errors,
                    "http_status",
                    "HTTP 状态码必须位于 100 到 599。",
                );
            }
        }
        BreakpointDecisionKind::WrongContentLength => {
            if decision.content_length_delta.is_none_or(|value| value == 0) {
                push_error(
                    &mut errors,
                    "content_length_delta",
                    "Content-Length 偏移量不能为 0。",
                );
            }
        }
        BreakpointDecisionKind::Truncate => {
            let available = detail.effective.body_bytes.len();
            if decision
                .truncate_at
                .is_none_or(|value| available == 0 || value >= available)
            {
                push_error(
                    &mut errors,
                    "truncate_at",
                    "截断位置必须位于 0 到当前 Body 字节数减 1 之间。",
                );
            }
        }
        BreakpointDecisionKind::ForwardOriginal
        | BreakpointDecisionKind::DisconnectBeforeUpstream
        | BreakpointDecisionKind::InvalidJson
        | BreakpointDecisionKind::DropResponse => {}
    }
    BreakpointValidationViewModel {
        valid: errors.is_empty(),
        field_errors: errors,
        warnings: Vec::new(),
    }
}

const fn valid_http_status(status: u16) -> bool {
    status >= 100 && status <= 599
}

fn validate_headers(headers: &BTreeMap<String, Vec<String>>) -> AppResult<()> {
    let mut errors = BTreeMap::<String, Vec<String>>::new();
    let mut content_length_count = 0;
    for (name, values) in headers {
        if name.parse::<HeaderName>().is_err() {
            push_error(
                &mut errors,
                "message.headers",
                "包含无效的 HTTP Header 名称。",
            );
        }
        if values.is_empty() {
            push_error(
                &mut errors,
                "message.headers",
                "HTTP Header 必须至少包含一个值。",
            );
        }
        for value in values {
            if HeaderValue::from_str(value).is_err() {
                push_error(
                    &mut errors,
                    "message.headers",
                    "包含无效的 HTTP Header 值。",
                );
            }
        }
        if name.eq_ignore_ascii_case("content-length") {
            content_length_count += 1;
            if values.len() != 1 || values[0].parse::<usize>().is_err() {
                push_error(
                    &mut errors,
                    "message.headers",
                    "Content-Length 必须只有一个非负整数字段值。",
                );
            }
        }
    }
    if content_length_count > 1 {
        push_error(
            &mut errors,
            "message.headers",
            "Content-Length 不得使用不同大小写重复声明。",
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(AppError::field(
            "HEADER_INVALID",
            "HTTP Header 校验失败。",
            errors,
        ))
    }
}

fn set_content_length(headers: &mut BTreeMap<String, Vec<String>>, length: usize) {
    let existing = headers
        .keys()
        .find(|name| name.eq_ignore_ascii_case("content-length"))
        .cloned()
        .unwrap_or_else(|| "content-length".into());
    let duplicates = headers
        .keys()
        .filter(|name| name.eq_ignore_ascii_case("content-length"))
        .cloned()
        .collect::<Vec<_>>();
    for duplicate in duplicates {
        headers.remove(&duplicate);
    }
    headers.insert(existing, vec![length.to_string()]);
}

pub(crate) const fn stage_supports_decision(
    stage: MessageStage,
    kind: BreakpointDecisionKind,
) -> bool {
    match stage {
        MessageStage::Request => matches!(
            kind,
            BreakpointDecisionKind::ForwardOriginal
                | BreakpointDecisionKind::ForwardModified
                | BreakpointDecisionKind::MockResponse
                | BreakpointDecisionKind::Delay
                | BreakpointDecisionKind::DisconnectBeforeUpstream
        ),
        MessageStage::Response => !matches!(
            kind,
            BreakpointDecisionKind::MockResponse | BreakpointDecisionKind::DisconnectBeforeUpstream
        ),
        MessageStage::TlsHandshake | MessageStage::Terminal => false,
    }
}

fn valid() -> BreakpointValidationViewModel {
    BreakpointValidationViewModel {
        valid: true,
        field_errors: BTreeMap::new(),
        warnings: Vec::new(),
    }
}

fn invalid(field: &str, message: &str) -> BreakpointValidationViewModel {
    BreakpointValidationViewModel {
        valid: false,
        field_errors: BTreeMap::from([(field.into(), vec![message.into()])]),
        warnings: Vec::new(),
    }
}

fn push_error(errors: &mut BTreeMap<String, Vec<String>>, field: &str, message: &str) {
    errors.entry(field.into()).or_default().push(message.into());
}

fn merge_errors(target: &mut BTreeMap<String, Vec<String>>, source: BTreeMap<String, Vec<String>>) {
    for (field, messages) in source {
        target.entry(field).or_default().extend(messages);
    }
}

#[cfg(test)]
#[path = "breakpoint_validation_tests.rs"]
mod tests;
