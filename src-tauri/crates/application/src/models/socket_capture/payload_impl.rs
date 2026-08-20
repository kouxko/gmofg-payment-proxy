use intercept_proxy_domain::ProtocolPackageRef;
use std::fmt;

use super::{
    SocketCaptureDocument, SocketCapturePayload, SocketLocalExchangeCapture,
    SocketLocalExchangeFailureCapture, SocketLocalExchangeFailureStage, SocketRelayFrameCapture,
    SocketRelayRuleStageCapture,
};

impl fmt::Debug for SocketRelayRuleStageCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketRelayRuleStageCapture")
            .field("stage", &self.stage)
            .field("matched_rule_count", &self.matched_rule_ids.len())
            .field("document_schema", self.document.schema.id())
            .finish()
    }
}

impl fmt::Debug for SocketRelayFrameCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketRelayFrameCapture")
            .field("direction", &self.direction)
            .field("package", &self.package)
            .field("schema", &self.schema)
            .field("origin_bytes", &self.origin.len())
            .field("stage_count", &self.stages.len())
            .field("written_bytes", &self.written.len())
            .field("display", &self.display)
            .finish()
    }
}

impl SocketRelayFrameCapture {
    const FIXED_OVERHEAD_BYTES: u64 = 192;

    fn logical_bytes(&self) -> u64 {
        Self::FIXED_OVERHEAD_BYTES
            + package_logical_bytes(&self.package)
            + self.schema.logical_bytes()
            + self.origin.len() as u64
            + self.written.len() as u64
            + self
                .stages
                .iter()
                .map(|stage| {
                    32 + stage.document.logical_bytes() + (stage.matched_rule_ids.len() as u64 * 16)
                })
                .sum::<u64>()
            + self.display.logical_bytes()
    }
}

impl fmt::Debug for SocketLocalExchangeCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketLocalExchangeCapture")
            .field("exchange_id", &self.exchange_id)
            .field("package", &self.package)
            .field("request_schema", &self.request_schema)
            .field("response_schema", &self.response_schema)
            .field("request_origin_bytes", &self.request_origin.len())
            .field("request_document_schema", self.request_document.schema.id())
            .field("request_display", &self.request_display)
            .field("matched_request_rule_ids", &self.matched_request_rule_ids)
            .field("matched_response_rule_ids", &self.matched_response_rule_ids)
            .field(
                "response_document_schema",
                self.response_document.schema.id(),
            )
            .field(
                "matched_request_rule_count",
                &self.matched_request_rule_ids.len(),
            )
            .field(
                "matched_response_rule_count",
                &self.matched_response_rule_ids.len(),
            )
            .field("written_response_bytes", &self.written_response.len())
            .field("response_display", &self.response_display)
            .finish()
    }
}

impl SocketLocalExchangeCapture {
    const FIXED_OVERHEAD_BYTES: u64 = 224;

    fn logical_bytes(&self) -> u64 {
        Self::FIXED_OVERHEAD_BYTES
            + package_logical_bytes(&self.package)
            + self.request_schema.logical_bytes()
            + self.response_schema.logical_bytes()
            + self.request_origin.len() as u64
            + self.written_response.len() as u64
            + self.request_document.logical_bytes()
            + self.request_display.logical_bytes()
            + self.response_document.logical_bytes()
            + (self.matched_request_rule_ids.len() as u64 * 16)
            + (self.matched_response_rule_ids.len() as u64 * 16)
            + self.response_display.logical_bytes()
    }
}

impl SocketLocalExchangeFailureStage {
    #[must_use]
    pub const fn stable_message(self) -> &'static str {
        match self {
            Self::ResponseRule => "代理→应用规则执行失败。",
            Self::ResponseEncode => "响应报文生成失败，请检查代理→应用规则是否补齐协议要求的字段。",
            Self::ResponseWrite => "响应写回应用失败，已保留请求解析结果和已写出的响应前缀。",
        }
    }
}

impl fmt::Debug for SocketLocalExchangeFailureCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketLocalExchangeFailureCapture")
            .field("exchange_id", &self.exchange_id)
            .field("package", &self.package)
            .field("request_schema", &self.request_schema)
            .field("response_schema", &self.response_schema)
            .field("request_origin_bytes", &self.request_origin.len())
            .field("request_document_schema", self.request_document.schema.id())
            .field("request_display", &self.request_display)
            .field("matched_request_rule_ids", &self.matched_request_rule_ids)
            .field("matched_response_rule_ids", &self.matched_response_rule_ids)
            .field(
                "response_document_schema",
                &self
                    .response_document
                    .as_ref()
                    .map(|value| value.schema.id()),
            )
            .field("failure_stage", &self.failure_stage)
            .field("failure_code", &self.failure_code)
            .field("failure_message_bytes", &self.failure_message.len())
            .field(
                "written_response_prefix_bytes",
                &self.written_response_prefix.len(),
            )
            .finish()
    }
}

impl SocketLocalExchangeFailureCapture {
    const FIXED_OVERHEAD_BYTES: u64 = 224;

    fn logical_bytes(&self) -> u64 {
        Self::FIXED_OVERHEAD_BYTES
            + package_logical_bytes(&self.package)
            + self.request_schema.logical_bytes()
            + self.response_schema.logical_bytes()
            + self.request_origin.len() as u64
            + self.request_document.logical_bytes()
            + self.request_display.logical_bytes()
            + self
                .response_document
                .as_ref()
                .map_or(0, SocketCaptureDocument::logical_bytes)
            + (self.matched_request_rule_ids.len() as u64 * 16)
            + (self.matched_response_rule_ids.len() as u64 * 16)
            + self.failure_code.len() as u64
            + self.failure_message.len() as u64
            + self.written_response_prefix.len() as u64
    }
}

impl fmt::Debug for SocketCapturePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelayFrame(value) => formatter.debug_tuple("RelayFrame").field(value).finish(),
            Self::LocalExchange(value) => {
                formatter.debug_tuple("LocalExchange").field(value).finish()
            }
            Self::LocalExchangeFailure(value) => formatter
                .debug_tuple("LocalExchangeFailure")
                .field(value)
                .finish(),
        }
    }
}

impl SocketCapturePayload {
    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        match self {
            Self::RelayFrame(value) => value.logical_bytes(),
            Self::LocalExchange(value) => value.logical_bytes(),
            Self::LocalExchangeFailure(value) => value.logical_bytes(),
        }
    }
}

fn package_logical_bytes(package: &ProtocolPackageRef) -> u64 {
    (package.id.as_str().len() + package.version.as_str().len()) as u64
}
