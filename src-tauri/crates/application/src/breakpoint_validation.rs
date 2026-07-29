use std::collections::BTreeMap;

use encoding_rs::SHIFT_JIS;
use http::{HeaderName, HeaderValue, StatusCode};

use crate::{
    AppError, AppResult, BreakpointDecision, BreakpointDecisionKind, BreakpointDetailViewModel,
    BreakpointDraft, BreakpointValidationPort, BreakpointValidationViewModel,
    MessageContentViewModel, MessageStage,
};

/// Canonical Rust-side validator and normalizer for breakpoint edits.
#[derive(Debug, Default)]
pub struct BreakpointValidator;

impl BreakpointValidator {
    fn normalize_message(
        mut message: MessageContentViewModel,
    ) -> AppResult<MessageContentViewModel> {
        validate_headers(&message.headers)?;
        if let Some(text) = message.body_text.as_deref() {
            let (encoded, _, had_errors) = SHIFT_JIS.encode(text);
            if had_errors {
                return Err(AppError::field(
                    "SHIFT_JIS_ENCODE_FAILED",
                    "报文包含 Shift-JIS 无法表示的字符。",
                    BTreeMap::from([(
                        "message.body_text".into(),
                        vec!["包含无法使用 Shift-JIS 编码的字符。".into()],
                    )]),
                ));
            }
            message.body_bytes = encoded.into_owned();
        }
        message.content_length = message.body_bytes.len();
        set_content_length(&mut message.headers, message.content_length);
        message.json = message
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
            })?;
        Ok(message)
    }

    fn validate_message(message: &MessageContentViewModel) -> BreakpointValidationViewModel {
        match Self::normalize_message(message.clone()) {
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
        draft.message = Self::normalize_message(draft.message)?;
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
        Ok(Self::validate_message(&draft.message))
    }

    fn validate_decision(
        &self,
        detail: &BreakpointDetailViewModel,
        decision: &BreakpointDecision,
    ) -> AppResult<BreakpointValidationViewModel> {
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
                match decision.message.as_ref() {
                    Some(message) => {
                        let validation = Self::validate_message(message);
                        merge_errors(&mut errors, validation.field_errors);
                    }
                    None => push_error(&mut errors, "message", "该操作必须提供报文。"),
                }
            }
            BreakpointDecisionKind::Delay => {
                if decision.delay_ms.is_none_or(|value| value == 0) {
                    push_error(&mut errors, "delay_ms", "延迟时间必须大于 0 毫秒。");
                }
            }
            BreakpointDecisionKind::CustomHttpStatus => {
                if decision
                    .http_status
                    .and_then(|value| StatusCode::from_u16(value).ok())
                    .is_none()
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
        Ok(BreakpointValidationViewModel {
            valid: errors.is_empty(),
            field_errors: errors,
            warnings: Vec::new(),
        })
    }
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
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    use crate::{BreakpointState, BreakpointSummaryViewModel, ChannelKind, UiTone};

    fn message(text: &str) -> MessageContentViewModel {
        MessageContentViewModel {
            headers: BTreeMap::from([("content-type".into(), vec!["application/json".into()])]),
            body_text: Some(text.into()),
            body_bytes: text.as_bytes().to_vec(),
            json: None,
            content_length: text.len(),
        }
    }

    fn detail(stage: MessageStage) -> BreakpointDetailViewModel {
        let original = message(r#"{"amount":100}"#);
        BreakpointDetailViewModel {
            summary: BreakpointSummaryViewModel {
                breakpoint_id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                runtime_epoch: Uuid::new_v4(),
                stage,
                title: String::new(),
                terminal_ip: "127.0.0.1".into(),
                channel: ChannelKind::Transaction,
                method: "POST".into(),
                target: "/pay".into(),
                waiting_since: Utc::now(),
                certificate_fingerprint_suffix: "1234".into(),
                state: BreakpointState::Pending,
                state_text: String::new(),
                ui_tone: UiTone::Warning,
                revision: 1,
            },
            original: original.clone(),
            effective: original,
            can_resolve: true,
            resolve_disabled_reason: None,
            available_actions: Vec::new(),
        }
    }

    #[test]
    fn format_json_reencodes_shift_jis_and_recalculates_length() {
        let detail = detail(MessageStage::Request);
        let draft = BreakpointDraft {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: 1,
            message: message(r#"{"result":"承認"}"#),
        };
        let formatted = BreakpointValidator
            .format_json(draft)
            .expect("format succeeds");
        assert_eq!(
            formatted.message.content_length,
            formatted.message.body_bytes.len()
        );
        assert_ne!(
            formatted.message.body_bytes,
            formatted.message.body_text.unwrap().into_bytes()
        );
    }

    #[test]
    fn rejects_unencodable_text_and_missing_truncate_position() {
        let detail = detail(MessageStage::Response);
        let draft = BreakpointDraft {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: 1,
            message: message(r#"{"value":"🧪"}"#),
        };
        assert!(
            !BreakpointValidator
                .validate(&detail, &draft)
                .expect("validation")
                .valid
        );

        let decision = BreakpointDecision {
            breakpoint_id: detail.summary.breakpoint_id,
            expected_revision: 1,
            kind: BreakpointDecisionKind::Truncate,
            message: None,
            delay_ms: None,
            http_status: None,
            content_length_delta: None,
            truncate_at: None,
        };
        assert!(
            !BreakpointValidator
                .validate_decision(&detail, &decision)
                .expect("validation")
                .valid
        );
        assert!(
            BreakpointValidator
                .validate_decision(
                    &detail,
                    &BreakpointDecision {
                        truncate_at: Some(0),
                        ..decision
                    },
                )
                .expect("zero position is valid")
                .valid
        );
    }
}
