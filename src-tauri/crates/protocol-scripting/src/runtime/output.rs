use std::sync::Arc;

use intercept_proxy_domain::Document;

use crate::ProtocolResourceLimit;

/// Decode/Encode 完成后已经确定的单 Frame 网络输出。
///
/// Decode 关闭时，执行器内部仍创建一个 Schema 绑定空 Document 供 Encode/Display 使用，但
/// [`ProtocolFrameOutput::decoded_document`] 返回 `None`，避免调用方误判为真正执行过 Decode。
#[derive(Clone, Debug)]
pub struct ProtocolFrameOutput {
    owner: Arc<()>,
    origin: Vec<u8>,
    written: Vec<u8>,
    decoded_document: Option<Document>,
    execution_document: Document,
}

impl ProtocolFrameOutput {
    pub(super) fn new(
        owner: Arc<()>,
        origin: Vec<u8>,
        written: Vec<u8>,
        decoded_document: Option<Document>,
        execution_document: Document,
    ) -> Self {
        Self {
            owner,
            origin,
            written,
            decoded_document,
            execution_document,
        }
    }

    /// 返回进入 Decode/Encode 前的完整原始 Frame。
    #[must_use]
    pub fn origin(&self) -> &[u8] {
        &self.origin
    }

    /// 返回应写入目标 Socket 的最终字节。
    #[must_use]
    pub fn written(&self) -> &[u8] {
        &self.written
    }

    /// Decode 开启时返回其 Schema 绑定 Document；关闭时返回 `None`。
    #[must_use]
    pub const fn decoded_document(&self) -> Option<&Document> {
        self.decoded_document.as_ref()
    }

    pub(super) const fn execution_document(&self) -> &Document {
        &self.execution_document
    }

    pub(super) fn belongs_to(&self, owner: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.owner, owner)
    }
}

/// Display 没有产生 HTML 时，UI 改为 Hex 展示的稳定原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayFallbackReason {
    /// 该方向 Encode 关闭；契约规定 Display 同时关闭。
    EncodeDisabled,
    /// Manifest 未声明公共 Display 入口。
    NotDeclared,
    /// Display 抛错或返回了非字符串值。
    EntryPointFailed,
    /// Display 触发了指定资源硬门禁。
    ResourceLimitExceeded(ProtocolResourceLimit),
}

/// 单 Frame 的后置 UI 展示结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolDisplayResult {
    /// 脚本返回的原始 HTML；它仍是不可信内容，应用层必须放入沙箱页面并施加 CSP。
    UntrustedHtml(String),
    /// UI 必须把 [`ProtocolFrameOutput::written`] 以 Hex 形式展示。
    HexFallback(DisplayFallbackReason),
}
