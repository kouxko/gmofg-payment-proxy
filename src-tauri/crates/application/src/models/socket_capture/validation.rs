//! 正式 Socket capture 的跨字段一致性校验。
//!
//! Serde 负责拒绝未知字段和错误类型；这里额外拒绝“形状合法但事实互相矛盾”的记录，
//! 仓储在写入和恢复后都会调用该校验，因此损坏 JSON 不能伪装成可信网络证据。

use std::collections::HashSet;

use super::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureRecord, SocketCaptureSchemaRef,
    SocketDisplayFallbackReason, SocketDisplayResult, SocketLocalExchangeFailureCapture,
    SocketLocalExchangeFailureStage, SocketRelayRuleStageCapture,
};
use intercept_proxy_domain::{ProtocolDirection, ProtocolDocumentRuleId, ProtocolRuleStage};

impl SocketCaptureRecord {
    /// 验证一条已完成 capture 的跨字段事实是否一致。
    ///
    /// 返回值刻意只有布尔值，避免把 Document、Frame 或 Display 内容拼入错误文本。
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        !self.peer_address.trim().is_empty()
            // Socket runtime 把“一条连接”作为唯一 Session；两个 ID 使用不同 DTO 类型，
            // 但底层 UUID 必须相同，避免查询与详情把同一证据分裂到两个 Session。
            && self.session_id == self.connection_id.as_uuid()
            && self.completed_at >= self.occurred_at
            && match &self.payload {
                SocketCapturePayload::RelayFrame(frame) => {
                    frame.schema.version > 0
                        && !frame.origin.is_empty()
                        && !frame.written.is_empty()
                        && relay_stages_match(frame.direction, &frame.stages, &frame.schema)
                        && display_matches_document(&frame.display)
                }
                SocketCapturePayload::LocalExchange(exchange) => {
                    exchange.request_schema.version > 0
                        && exchange.response_schema.version > 0
                        && !exchange.request_origin.is_empty()
                        && !exchange.written_response.is_empty()
                        && schema_matches(&exchange.request_document, &exchange.request_schema)
                        && display_matches_document(&exchange.request_display)
                        && schema_matches(&exchange.response_document, &exchange.response_schema)
                        && display_matches_document(&exchange.response_display)
                        && unique_rule_ids(&exchange.matched_request_rule_ids)
                        && unique_rule_ids(&exchange.matched_response_rule_ids)
                }
                SocketCapturePayload::LocalExchangeFailure(failure) => {
                    failed_exchange_is_consistent(failure)
                }
            }
    }
}

fn failed_exchange_is_consistent(failure: &SocketLocalExchangeFailureCapture) -> bool {
    failure.request_schema.version > 0
        && failure.response_schema.version > 0
        && !failure.request_origin.is_empty()
        && schema_matches(&failure.request_document, &failure.request_schema)
        && display_matches_document(&failure.request_display)
        && failure
            .response_document
            .as_ref()
            .is_none_or(|document| schema_matches(document, &failure.response_schema))
        && unique_rule_ids(&failure.matched_request_rule_ids)
        && unique_rule_ids(&failure.matched_response_rule_ids)
        && failure.failure_message == failure.failure_stage.stable_message()
        && failure_code_matches_stage(failure.failure_stage, &failure.failure_code)
        && (failure.failure_stage == SocketLocalExchangeFailureStage::ResponseWrite
            || failure.written_response_prefix.is_empty())
        && (failure.written_response_prefix.is_empty() || failure.response_document.is_some())
}

fn failure_code_matches_stage(stage: SocketLocalExchangeFailureStage, code: &str) -> bool {
    match stage {
        SocketLocalExchangeFailureStage::ResponseRule => code == "RULE_FAILED",
        SocketLocalExchangeFailureStage::ResponseEncode => matches!(
            code,
            "ENCODE_FAILED"
                | "EMPTY_OUTPUT"
                | "OUTPUT_LIMIT_EXCEEDED"
                | "PROCESSING_FAILED"
                | "PROCESSING_TIMEOUT"
        ),
        SocketLocalExchangeFailureStage::ResponseWrite => {
            matches!(code, "WRITE_FAILED" | "WRITE_TIMEOUT" | "CANCELLED")
        }
    }
}

fn relay_stages_match(
    direction: ProtocolDirection,
    stages: &[SocketRelayRuleStageCapture],
    schema: &SocketCaptureSchemaRef,
) -> bool {
    let expected = match direction {
        ProtocolDirection::Upstream => [
            ProtocolRuleStage::AppToProxy,
            ProtocolRuleStage::ProxyToUpstream,
        ],
        ProtocolDirection::Downstream => [
            ProtocolRuleStage::UpstreamToProxy,
            ProtocolRuleStage::ProxyToApp,
        ],
    };
    stages.len() == expected.len()
        && stages.iter().zip(expected).all(|(snapshot, stage)| {
            snapshot.stage == stage
                && schema_matches(&snapshot.document, schema)
                && unique_rule_ids(&snapshot.matched_rule_ids)
        })
}

fn schema_matches(document: &SocketCaptureDocument, expected: &SocketCaptureSchemaRef) -> bool {
    expected.version > 0
        && document.is_consistent_with_schema(expected.id.as_str(), expected.version)
}

fn display_matches_document(display: &SocketDisplayResult) -> bool {
    matches!(
        display,
        SocketDisplayResult::UntrustedHtml { .. }
            | SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EntryPointFailed
                    | SocketDisplayFallbackReason::ResourceLimitExceeded,
                ..
            }
    )
}

fn unique_rule_ids(ids: &[ProtocolDocumentRuleId]) -> bool {
    let mut unique = HashSet::with_capacity(ids.len());
    ids.iter().all(|id| unique.insert(*id))
}
