use std::fmt;

use crate::ProductError;

/// 宿主选择的 HTTP 正文编解码契约，例如 UTF-8 或 Shift-JIS。
pub trait BodyCodec: fmt::Debug + Send + Sync {
    /// 稳定编解码器 ID，便于日志和诊断识别。
    fn id(&self) -> &'static str;

    /// 给人阅读的编码名称。
    fn name(&self) -> &'static str;

    /// 将线上字节无损解码为可编辑文本；存在非法字节时必须失败。
    fn decode(&self, bytes: &[u8]) -> Result<String, ProductError>;

    /// 将用户文本无损编码回线上字节；有不可表示字符时必须失败。
    fn encode(&self, text: &str) -> Result<Vec<u8>, ProductError>;
}
