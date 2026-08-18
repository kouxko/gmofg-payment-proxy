//! Socket Frame 与本地应答 exchange 的持久化查询模型。
//!
//! 这些 DTO 与 HTTP 抓包模型完全分离：它们不包含 start line、Header、状态码或
//! JSONPath。完整捕获只在网络写出成功后创建；处理中失败只能使用本模块末尾的脱敏
//! diagnostic，不能伪造成一条已完成 capture。

use chrono::{DateTime, Utc};
use intercept_proxy_domain::{
    DocumentSchemaId, ListenerId, ProtocolPackageRef, SocketDirection, SocketDocumentRuleId,
    WorkspaceId,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fmt;
use uuid::Uuid;

use super::{PageRequest, RuntimeEpoch, SessionId, SortDirection};

mod document;
mod payload_wire;
mod validation;
pub use document::*;

macro_rules! socket_uuid_id {
    ($name:ident, $comment:literal) => {
        #[doc = $comment]
        #[derive(
            Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize, Type,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

socket_uuid_id!(SocketCaptureId, "一条已完成 Socket capture 的稳定标识。");
socket_uuid_id!(
    SocketExchangeId,
    "一次 `LocalResponder` request/response 原子交换的稳定标识。"
);
socket_uuid_id!(
    SocketConnectionId,
    "一条 Socket 连接的稳定标识；同一运行周期内不得复用。"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 捕获时实际使用的精确 Schema 身份。
pub struct SocketCaptureSchemaRef {
    pub id: DocumentSchemaId,
    pub version: u32,
}

impl SocketCaptureSchemaRef {
    const FIXED_OVERHEAD_BYTES: u64 = 16;

    fn logical_bytes(&self) -> u64 {
        Self::FIXED_OVERHEAD_BYTES + self.id.as_str().len() as u64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// Hex fallback 的稳定原因；不保存脚本异常文本或原始 payload。
pub enum SocketDisplayFallbackReason {
    EncodeDisabled,
    NotDeclared,
    EntryPointFailed,
    ResourceLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// Display 失败时允许持久化的脱敏诊断。
pub struct SocketDisplayDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
/// 协议展示结果。HTML 即使经过脚本生成也仍按不可信内容处理。
pub enum SocketDisplayResult {
    UntrustedHtml {
        html: String,
    },
    HexFallback {
        reason: SocketDisplayFallbackReason,
        diagnostic: Option<SocketDisplayDiagnostic>,
    },
}

impl<'de> Deserialize<'de> for SocketDisplayResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct HtmlFields {
            html: String,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FallbackFields {
            reason: SocketDisplayFallbackReason,
            diagnostic: Option<SocketDisplayDiagnostic>,
        }

        #[derive(Deserialize)]
        #[serde(tag = "type", rename_all = "snake_case")]
        enum Wire {
            UntrustedHtml(HtmlFields),
            HexFallback(FallbackFields),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::UntrustedHtml(fields) => Self::UntrustedHtml { html: fields.html },
            Wire::HexFallback(fields) => Self::HexFallback {
                reason: fields.reason,
                diagnostic: fields.diagnostic,
            },
        })
    }
}

impl fmt::Debug for SocketDisplayResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UntrustedHtml { html } => formatter
                .debug_struct("UntrustedHtml")
                .field("html_bytes", &html.len())
                .finish(),
            Self::HexFallback { reason, diagnostic } => formatter
                .debug_struct("HexFallback")
                .field("reason", reason)
                .field(
                    "diagnostic_code",
                    &diagnostic.as_ref().map(|value| &value.code),
                )
                .finish(),
        }
    }
}

impl SocketDisplayResult {
    fn logical_bytes(&self) -> u64 {
        const FIXED_OVERHEAD_BYTES: u64 = 24;
        FIXED_OVERHEAD_BYTES
            + match self {
                Self::UntrustedHtml { html } => html.len() as u64,
                Self::HexFallback { diagnostic, .. } => diagnostic
                    .as_ref()
                    .map_or(0, |value| (value.code.len() + value.message.len()) as u64),
            }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
/// `written` 字节来自原文还是 Encode；UI 不通过比较字节猜测处理路径。
pub enum SocketWriteKind {
    Original,
    Encoded,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// Relay 中一个方向已成功写出的完整 Frame。
pub struct SocketRelayFrameCapture {
    pub direction: SocketDirection,
    pub package: ProtocolPackageRef,
    pub schema: SocketCaptureSchemaRef,
    pub decode_enabled: bool,
    pub encode_enabled: bool,
    /// 网络读取到的完整原始 Frame；Decode 关闭或失败时仍必须保留。
    pub origin: Vec<u8>,
    /// 规则执行所使用的 Document。Decode 关闭时必须为 `None`。
    pub document: Option<SocketCaptureDocument>,
    pub matched_rule_ids: Vec<SocketDocumentRuleId>,
    /// 实际成功写入另一端的完整字节；不得记录部分写入缓冲区。
    pub written: Vec<u8>,
    pub write_kind: SocketWriteKind,
    pub display: SocketDisplayResult,
}

impl fmt::Debug for SocketRelayFrameCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketRelayFrameCapture")
            .field("direction", &self.direction)
            .field("package", &self.package)
            .field("schema", &self.schema)
            .field("decode_enabled", &self.decode_enabled)
            .field("encode_enabled", &self.encode_enabled)
            .field("origin_bytes", &self.origin.len())
            .field("document_present", &self.document.is_some())
            .field("matched_rule_count", &self.matched_rule_ids.len())
            .field("written_bytes", &self.written.len())
            .field("write_kind", &self.write_kind)
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
            + document_logical_bytes(self.document.as_ref())
            + (self.matched_rule_ids.len() as u64 * 16)
            + self.display.logical_bytes()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// `LocalResponder` 中一次完整、已写出的 request/response exchange。
pub struct SocketLocalExchangeCapture {
    pub exchange_id: SocketExchangeId,
    pub package: ProtocolPackageRef,
    pub schema: SocketCaptureSchemaRef,
    pub request_decode_enabled: bool,
    pub response_encode_enabled: bool,
    pub request_origin: Vec<u8>,
    /// Decode 关闭时必须保持 `None`，不得合成空 Document 冒充解析结果。
    pub request_document: Option<SocketCaptureDocument>,
    /// Decode 关闭时为 `None`；成功 Decode 后保存同一协议包的 Display 或明确 Hex 回退。
    pub request_display: Option<SocketDisplayResult>,
    /// 规则只修改该响应 Document，不得覆盖 request Document。
    pub response_document: SocketCaptureDocument,
    pub matched_downstream_rule_ids: Vec<SocketDocumentRuleId>,
    pub written_response: Vec<u8>,
    pub response_write_kind: SocketWriteKind,
    pub response_display: SocketDisplayResult,
}

impl fmt::Debug for SocketLocalExchangeCapture {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketLocalExchangeCapture")
            .field("exchange_id", &self.exchange_id)
            .field("package", &self.package)
            .field("schema", &self.schema)
            .field("request_decode_enabled", &self.request_decode_enabled)
            .field("response_encode_enabled", &self.response_encode_enabled)
            .field("request_origin_bytes", &self.request_origin.len())
            .field("request_document_present", &self.request_document.is_some())
            .field("request_display", &self.request_display)
            .field(
                "response_document_schema",
                self.response_document.schema.id(),
            )
            .field(
                "matched_downstream_rule_count",
                &self.matched_downstream_rule_ids.len(),
            )
            .field("written_response_bytes", &self.written_response.len())
            .field("response_write_kind", &self.response_write_kind)
            .field("response_display", &self.response_display)
            .finish()
    }
}

impl SocketLocalExchangeCapture {
    const FIXED_OVERHEAD_BYTES: u64 = 224;

    fn logical_bytes(&self) -> u64 {
        Self::FIXED_OVERHEAD_BYTES
            + package_logical_bytes(&self.package)
            + self.schema.logical_bytes()
            + self.request_origin.len() as u64
            + self.written_response.len() as u64
            + document_logical_bytes(self.request_document.as_ref())
            + self
                .request_display
                .as_ref()
                .map_or(0, SocketDisplayResult::logical_bytes)
            + document_logical_bytes(Some(&self.response_document))
            + (self.matched_downstream_rule_ids.len() as u64 * 16)
            + self.response_display.logical_bytes()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", content = "capture", rename_all = "snake_case")]
/// Socket capture 的封闭联合类型；本地 exchange 不伪装为 Server frame。
pub enum SocketCapturePayload {
    RelayFrame(SocketRelayFrameCapture),
    LocalExchange(SocketLocalExchangeCapture),
}

impl fmt::Debug for SocketCapturePayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelayFrame(value) => formatter.debug_tuple("RelayFrame").field(value).finish(),
            Self::LocalExchange(value) => {
                formatter.debug_tuple("LocalExchange").field(value).finish()
            }
        }
    }
}

impl SocketCapturePayload {
    #[must_use]
    pub fn logical_bytes(&self) -> u64 {
        match self {
            Self::RelayFrame(value) => value.logical_bytes(),
            Self::LocalExchange(value) => value.logical_bytes(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 仓储保存的完整 Socket capture 聚合根。
pub struct SocketCaptureRecord {
    pub capture_id: SocketCaptureId,
    pub runtime_epoch: RuntimeEpoch,
    pub workspace_id: WorkspaceId,
    pub listener_id: ListenerId,
    pub session_id: SessionId,
    pub connection_id: SocketConnectionId,
    pub peer_address: String,
    pub occurred_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub payload: SocketCapturePayload,
}

impl fmt::Debug for SocketCaptureRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketCaptureRecord")
            .field("capture_id", &self.capture_id)
            .field("runtime_epoch", &self.runtime_epoch)
            .field("workspace_id", &self.workspace_id)
            .field("listener_id", &self.listener_id)
            .field("session_id", &self.session_id)
            .field("connection_id", &self.connection_id)
            .field("peer_address", &self.peer_address)
            .field("occurred_at", &self.occurred_at)
            .field("completed_at", &self.completed_at)
            .field("payload", &self.payload)
            .finish()
    }
}

impl SocketCaptureRecord {
    pub const ENTITY_FIXED_OVERHEAD_BYTES: u64 = 160;

    #[must_use]
    /// 返回仓储配额使用的逻辑字节数，不依赖 `Vec` capacity 或平台指针宽度。
    pub fn logical_bytes(&self) -> u64 {
        Self::ENTITY_FIXED_OVERHEAD_BYTES
            + self.peer_address.len() as u64
            + self.payload.logical_bytes()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketCaptureKind {
    RelayFrame,
    LocalExchange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SocketCaptureSort {
    OccurredAt,
    CompletedAt,
    Size,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// Socket 页面专用查询；刻意不接受 HTTP stage、status、Header 或 `JSONPath` 条件。
pub struct SocketCaptureQuery {
    pub workspace_id: Option<WorkspaceId>,
    pub listener_id: Option<ListenerId>,
    pub session_id: Option<SessionId>,
    pub connection_id: Option<SocketConnectionId>,
    pub package: Option<ProtocolPackageRef>,
    pub direction: Option<SocketDirection>,
    pub kind: Option<SocketCaptureKind>,
    pub occurred_from: Option<DateTime<Utc>>,
    pub occurred_to: Option<DateTime<Utc>>,
    pub sort: SocketCaptureSort,
    pub direction_sort: SortDirection,
    pub page: PageRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 分页列表中的轻量行，不重复返回 Document、HTML 或完整字节。
pub struct SocketCaptureRowViewModel {
    pub capture_id: SocketCaptureId,
    pub runtime_epoch: RuntimeEpoch,
    pub session_id: SessionId,
    pub connection_id: SocketConnectionId,
    pub listener_id: ListenerId,
    pub occurred_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub kind: SocketCaptureKind,
    pub direction: Option<SocketDirection>,
    pub package: ProtocolPackageRef,
    pub schema: SocketCaptureSchemaRef,
    pub origin_size_bytes: u64,
    pub written_size_bytes: u64,
    pub logical_size_bytes: u64,
    pub matched_rule_ids: Vec<SocketDocumentRuleId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct SocketCapturePageViewModel {
    pub rows: Vec<SocketCaptureRowViewModel>,
    pub total: usize,
    pub page: u32,
    pub page_size: u32,
    pub total_pages: u32,
    pub empty_message: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 完整详情；`record` 是网络写出成功后的原子证据。
pub struct SocketCaptureDetailViewModel {
    pub record: SocketCaptureRecord,
}

impl fmt::Debug for SocketCaptureDetailViewModel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketCaptureDetailViewModel")
            .field("record", &self.record)
            .finish()
    }
}

fn package_logical_bytes(package: &ProtocolPackageRef) -> u64 {
    (package.id.as_str().len() + package.version.as_str().len()) as u64
}

fn document_logical_bytes(document: Option<&SocketCaptureDocument>) -> u64 {
    document.map_or(0, |value| {
        // Document 的严格 wire 包含完整 Schema 和稀疏值槽。使用 wire 字节长度可稳定计入
        // 字段名、标签、String/Blob 内容，且不会受 allocator capacity 或平台 ABI 影响。
        serde_json::to_vec(value).map_or(u64::MAX, |bytes| bytes.len() as u64)
    })
}
