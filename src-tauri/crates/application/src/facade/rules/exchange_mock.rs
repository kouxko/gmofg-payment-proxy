use intercept_proxy_domain::{
    Condition, HttpAction, HttpRuleContent, MatchField, MatchOperator, ProtocolDirection,
    UnifiedAction,
};

use super::Application;
use crate::{
    AppError, AppResult, ExchangeContext, ExchangeObservationEvent, ExchangeObservationRecord,
    ExchangeProtocol, RuleContent, RuleDefinition, RuleDefinitionDraft, RuleDefinitionSaveInput,
    RuleStage,
};

const INVALID_SOURCE: &str = "HTTP_MOCK_DRAFT_SOURCE_INVALID";

impl Application {
    /// Builds an unsaved unified HTTP rule draft from a complete captured response.
    pub fn rule_definition_create_from_exchange_observation(
        &self,
        record: &ExchangeObservationRecord,
        response_event_index: usize,
    ) -> AppResult<RuleDefinitionSaveInput> {
        let source = MockDraftSource::from_record(record, response_event_index)?;
        let unified = source.into_unified_input(record)?;
        RuleDefinition::create(unified.draft.clone(), 1)?;
        Ok(unified)
    }
}

struct MockDraftSource<'a> {
    request_target: String,
    response_body: &'a str,
    response_body_is_utf8: bool,
}

impl<'a> MockDraftSource<'a> {
    fn from_record(
        record: &'a ExchangeObservationRecord,
        response_event_index: usize,
    ) -> AppResult<Self> {
        if record.protocol != ExchangeProtocol::Http || record.evidence_evicted {
            return Err(source_error(
                "抓包证据不是完整 HTTP 交换，无法生成 Mock 规则草稿。",
            ));
        }
        let (response_body, response_body_is_utf8) = match record.events.get(response_event_index) {
            Some(ExchangeObservationEvent::Received {
                direction: ProtocolDirection::Downstream,
                context:
                    ExchangeContext::Http {
                        header: _,
                        body,
                        body_is_utf8,
                    },
                ..
            }) => (body.as_str(), *body_is_utf8),
            _ => {
                return Err(source_error("所选事件不是服务器返回的完整 HTTP 响应。"));
            }
        };
        let request_header = record.events[..response_event_index]
            .iter()
            .rev()
            .find_map(|event| match event {
                ExchangeObservationEvent::Sent {
                    direction: ProtocolDirection::Upstream,
                    context: ExchangeContext::Http { header, .. },
                    ..
                } => Some(header.as_str()),
                _ => None,
            })
            .ok_or_else(|| source_error("服务器响应缺少可配对的 HTTP 请求。"))?;
        Ok(Self {
            request_target: request_target(request_header)?,
            response_body,
            response_body_is_utf8,
        })
    }

    fn into_unified_input(
        self,
        record: &ExchangeObservationRecord,
    ) -> AppResult<RuleDefinitionSaveInput> {
        if !self.response_body_is_utf8 {
            return Err(AppError::new(
                "HTTP_MOCK_DRAFT_BODY_NOT_UTF8",
                "服务器响应 Body 不是 UTF-8 文本，无法无损生成 Mock 规则草稿。",
            ));
        }
        Ok(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft: RuleDefinitionDraft {
                name: format!("Mock {}", self.request_target),
                enabled: false,
                priority: 100,
                listener_id: record.listener_id,
                stage: RuleStage::ProxyToApp,
                content: RuleContent::Http(HttpRuleContent {
                    description: format!(
                        "由 HTTP 抓包 {} 的服务器响应 Body 生成，需配合 LocalHttpServer。",
                        record.exchange_id
                    ),
                    condition: Condition::Http {
                        field: MatchField::RequestTarget,
                        operator: MatchOperator::Equals(self.request_target),
                    },
                    action: UnifiedAction::Http(HttpAction::ReplaceBodyText(
                        self.response_body.to_owned(),
                    )),
                }),
            },
        })
    }
}

fn request_target(header: &str) -> AppResult<String> {
    let line = header
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('\r');
    let mut parts = line.split_whitespace();
    let _method = parts.next();
    let target = parts.next();
    let version = parts.next();
    if target.is_none() || version.is_none_or(|value| !value.starts_with("HTTP/")) {
        return Err(source_error("配对请求的 HTTP request-line 无效。"));
    }
    Ok(target.expect("checked target").to_owned())
}

fn source_error(message: &str) -> AppError {
    AppError::new(INVALID_SOURCE, message)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use intercept_proxy_domain::{
        HttpAction, ListenerId, ProtocolDirection, UnifiedAction, WorkspaceId,
    };
    use uuid::Uuid;

    use super::MockDraftSource;
    use crate::{
        ExchangeContext, ExchangeObservationEvent, ExchangeObservationRecord, ExchangeProtocol,
        RuleContent, RuleStage,
    };

    #[test]
    fn captured_response_body_becomes_proxy_to_app_replace_body() {
        let record = ExchangeObservationRecord {
            exchange_id: "exchange-1".into(),
            workspace_id: WorkspaceId::new(),
            listener_id: ListenerId::new(),
            runtime_epoch: Uuid::nil(),
            peer_address: "127.0.0.1:12345".into(),
            protocol: ExchangeProtocol::Http,
            events: vec![
                ExchangeObservationEvent::Sent {
                    observed_at: Utc::now(),
                    direction: ProtocolDirection::Upstream,
                    context: ExchangeContext::Http {
                        header: "POST /payment?attempt=1 HTTP/1.1\r\nHost: example.test\r\n\r\n"
                            .into(),
                        body: String::new(),
                        body_is_utf8: true,
                    },
                },
                ExchangeObservationEvent::Received {
                    observed_at: Utc::now(),
                    direction: ProtocolDirection::Downstream,
                    context: ExchangeContext::Http {
                        header: "HTTP/1.1 201 Created\r\nX-Trace: ignored\r\n\r\n".into(),
                        body: "mock body".into(),
                        body_is_utf8: true,
                    },
                    document: None,
                    display: None,
                },
            ],
            evidence_evicted: false,
        };

        let source = MockDraftSource::from_record(&record, 1).expect("captured response source");
        let input = source
            .into_unified_input(&record)
            .expect("replace body draft");

        assert_eq!(input.draft.stage, RuleStage::ProxyToApp);
        let RuleContent::Http(content) = input.draft.content else {
            panic!("HTTP rule content expected");
        };
        assert!(matches!(
            content.action,
            UnifiedAction::Http(HttpAction::ReplaceBodyText(ref body)) if body == "mock body"
        ));
    }
}
