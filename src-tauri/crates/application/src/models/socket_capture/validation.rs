//! 正式 Socket capture 的跨字段一致性校验。
//!
//! Serde 负责拒绝未知字段和错误类型；这里额外拒绝“形状合法但事实互相矛盾”的记录，
//! 例如 Decode 关闭却携带 Document，或 Encode 关闭却标记为 Encoded。仓储在写入和恢复
//! 后都会调用该校验，因此损坏 JSON 不能伪装成可信网络证据。

use std::collections::HashSet;

use super::{
    SocketCaptureDocument, SocketCapturePayload, SocketCaptureRecord, SocketCaptureSchemaRef,
    SocketDisplayFallbackReason, SocketDisplayResult, SocketWriteKind,
};
use intercept_proxy_domain::SocketDocumentRuleId;

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
                        && frame.document.is_some() == frame.decode_enabled
                        && frame
                            .document
                            .as_ref()
                            .is_none_or(|document| schema_matches(document, &frame.schema))
                        && write_kind_matches(frame.encode_enabled, frame.write_kind)
                        && (frame.decode_enabled || frame.matched_rule_ids.is_empty())
                        && raw_write_matches(
                            frame.encode_enabled,
                            &frame.origin,
                            &frame.written,
                        )
                        && display_matches(frame.encode_enabled, &frame.display)
                        && unique_rule_ids(&frame.matched_rule_ids)
                }
                SocketCapturePayload::LocalExchange(exchange) => {
                    exchange.schema.version > 0
                        && !exchange.request_origin.is_empty()
                        && !exchange.written_response.is_empty()
                        && exchange.request_document.is_some() == exchange.request_decode_enabled
                        && exchange
                            .request_document
                            .as_ref()
                            .is_none_or(|document| schema_matches(document, &exchange.schema))
                        && schema_matches(&exchange.response_document, &exchange.schema)
                        && write_kind_matches(
                            exchange.response_encode_enabled,
                            exchange.response_write_kind,
                        )
                        && raw_write_matches(
                            exchange.response_encode_enabled,
                            &exchange.request_origin,
                            &exchange.written_response,
                        )
                        && display_matches(
                            exchange.response_encode_enabled,
                            &exchange.response_display,
                        )
                        && unique_rule_ids(&exchange.matched_downstream_rule_ids)
                }
            }
    }
}

fn schema_matches(document: &SocketCaptureDocument, expected: &SocketCaptureSchemaRef) -> bool {
    expected.version > 0
        && document.is_consistent_with_schema(expected.id.as_str(), expected.version)
}

const fn write_kind_matches(enabled: bool, actual: SocketWriteKind) -> bool {
    matches!(
        (enabled, actual),
        (true, SocketWriteKind::Encoded) | (false, SocketWriteKind::Original)
    )
}

fn raw_write_matches(enabled: bool, origin: &[u8], written: &[u8]) -> bool {
    enabled || origin == written
}

fn display_matches(enabled: bool, display: &SocketDisplayResult) -> bool {
    if !enabled {
        return matches!(
            display,
            SocketDisplayResult::HexFallback {
                reason: SocketDisplayFallbackReason::EncodeDisabled,
                diagnostic: None,
            }
        );
    }
    !matches!(
        display,
        SocketDisplayResult::HexFallback {
            reason: SocketDisplayFallbackReason::EncodeDisabled,
            ..
        }
    )
}

fn unique_rule_ids(ids: &[SocketDocumentRuleId]) -> bool {
    let mut unique = HashSet::with_capacity(ids.len());
    ids.iter().all(|id| unique.insert(*id))
}
