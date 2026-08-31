use std::collections::BTreeSet;

use http::{HeaderName, HeaderValue};
use intercept_proxy_domain::{
    Condition, ConditionTree, HttpRuleContent, MatchField, MatchOperator, ProtocolDirection,
    TerminalAction, UnifiedAction,
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
    response_header: &'a str,
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
        let (response_header, response_body, response_body_is_utf8) =
            match record.events.get(response_event_index) {
                Some(ExchangeObservationEvent::Received {
                    direction: ProtocolDirection::Downstream,
                    context:
                        ExchangeContext::Http {
                            header,
                            body,
                            body_is_utf8,
                        },
                    ..
                }) => (header.as_str(), body.as_str(), *body_is_utf8),
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
            response_header,
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
        let (status, headers) = response_metadata(self.response_header)?;
        Ok(RuleDefinitionSaveInput {
            rule_id: None,
            expected_revision: None,
            draft: RuleDefinitionDraft {
                name: format!("Mock {}", self.request_target),
                enabled: false,
                priority: 100,
                listener_id: record.listener_id,
                stage: RuleStage::ProxyToUpstream,
                one_shot: false,
                content: RuleContent::Http(HttpRuleContent {
                    description: format!("由 HTTP 抓包 {} 的服务器响应生成。", record.exchange_id),
                    condition: ConditionTree::Leaf(Condition::Http {
                        field: MatchField::RequestTarget,
                        operator: MatchOperator::Equals(self.request_target),
                    }),
                    actions: vec![UnifiedAction::Terminal(TerminalAction::MockResponse {
                        status,
                        headers,
                        body_bytes: self.response_body.as_bytes().to_vec(),
                    })],
                    document: None,
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

fn response_metadata(header: &str) -> AppResult<(u16, Vec<(String, String)>)> {
    let mut lines = header.lines();
    let status_line = lines.next().unwrap_or_default().trim_end_matches('\r');
    let mut status_parts = status_line.split_whitespace();
    let version = status_parts.next();
    let status = status_parts
        .next()
        .and_then(|value| value.parse::<u16>().ok());
    if version.is_none_or(|value| !value.starts_with("HTTP/")) || status.is_none() {
        return Err(source_error("服务器响应的 HTTP status-line 无效。"));
    }

    let parsed = parse_headers(lines)?;
    reject_encoded_body(&parsed)?;
    let connection_tokens = connection_tokens(&parsed);
    let headers = parsed
        .into_iter()
        .filter(|(name, _)| !is_hop_by_hop(name) && !connection_tokens.contains(name))
        .filter(|(name, _)| name != "content-length")
        .collect::<Vec<_>>();
    Ok((status.expect("checked status"), headers))
}

fn parse_headers<'a>(lines: impl Iterator<Item = &'a str>) -> AppResult<Vec<(String, String)>> {
    let mut headers = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        let (raw_name, raw_value) = line
            .split_once(':')
            .ok_or_else(|| source_error("服务器响应包含无效 HTTP Header。"))?;
        let name = raw_name.trim().to_ascii_lowercase();
        let value = raw_value.trim();
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| source_error("服务器响应包含无效 HTTP Header 名称。"))?;
        HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| source_error("服务器响应包含无效 HTTP Header 值。"))?;
        headers.push((name, value.to_owned()));
    }
    Ok(headers)
}

fn reject_encoded_body(headers: &[(String, String)]) -> AppResult<()> {
    let unsupported = headers
        .iter()
        .filter(|(name, _)| name == "content-encoding")
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .any(|value| !value.is_empty() && !value.eq_ignore_ascii_case("identity"));
    if unsupported {
        Err(AppError::new(
            "HTTP_MOCK_DRAFT_BODY_ENCODED",
            "服务器响应使用了压缩或其他 Content-Encoding，无法安全生成 Mock 规则草稿。",
        ))
    } else {
        Ok(())
    }
}

fn connection_tokens(headers: &[(String, String)]) -> BTreeSet<String> {
    headers
        .iter()
        .filter(|(name, _)| name == "connection")
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-connection"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn source_error(message: &str) -> AppError {
    AppError::new(INVALID_SOURCE, message)
}
