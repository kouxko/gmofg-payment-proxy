//! Exchange Pipeline 使用的 Rhai 单阶段入口。

use intercept_proxy_domain::Document;

use crate::{ProtocolEntryPoint, ProtocolRuntimeResult};

use super::ProtocolDirectionExecutor;

impl ProtocolDirectionExecutor {
    /// 只运行当前方向的 Decode，并返回 Schema 绑定的真实 Document。
    pub fn decode_document(&mut self, origin: &[u8]) -> ProtocolRuntimeResult<Document> {
        self.ensure_blob_input(ProtocolEntryPoint::Decode, origin.len())?;
        self.call_decode(origin)
    }

    /// 只运行当前方向的 Encode，把 Rules 返回的 owned Document 转成线路字节。
    pub fn encode_document(
        &mut self,
        original: &[u8],
        mut document: Document,
    ) -> ProtocolRuntimeResult<Vec<u8>> {
        self.ensure_blob_input(ProtocolEntryPoint::Encode, original.len())?;
        self.call_encode(original, &mut document)
    }

    /// 只运行当前方向的 Display；错误由 Reader Pipeline 统一生成 Hex fallback。
    pub fn display_document(&mut self, document: &Document) -> ProtocolRuntimeResult<String> {
        self.call_display(document)
    }
}
