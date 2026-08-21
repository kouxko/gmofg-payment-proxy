use crate::{DomainError, ErrorCode, ExternalDocumentWire};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use specta::Type;

fn decode_canonical_base64(field: &str, value: &str) -> Result<Vec<u8>, DomainError> {
    let decoded = STANDARD.decode(value.as_bytes()).map_err(|_| {
        DomainError::new(ErrorCode::BodyDecodeFailed, "外部软件包返回了非法 Base64")
            .with_field_error(field, "必须是带标准填充的 canonical Base64")
    })?;
    if STANDARD.encode(&decoded) != value {
        return Err(
            DomainError::new(ErrorCode::BodyDecodeFailed, "外部软件包返回了非规范 Base64")
                .with_field_error(field, "必须是带标准填充的 canonical Base64"),
        );
    }
    Ok(decoded)
}

/// `frame` 调用的参数；包含当前方向完整累积缓冲区。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalFrameRequest {
    buffer_base64: String,
}

impl ExternalFrameRequest {
    /// 将原始累积缓冲区编码为 canonical Base64 参数。
    #[must_use]
    pub fn from_bytes(buffer: &[u8]) -> Self {
        Self {
            buffer_base64: STANDARD.encode(buffer),
        }
    }

    /// 校验并解码累积缓冲区。
    pub fn bytes(&self) -> Result<Vec<u8>, DomainError> {
        decode_canonical_base64("buffer_base64", &self.buffer_base64)
    }
}

/// `frame` 调用的严格 closed-union 结果。
#[derive(Clone, Debug, Eq, PartialEq, Type)]
pub enum ExternalFrameResult {
    /// 当前累积缓冲区尚不足一帧，Proxy 应继续读取。
    NeedMore,
    /// 缓冲区前缀已形成完整帧；字节数必须大于零。
    Complete {
        /// 从累积缓冲区头部切出的字节数。
        consumed_bytes: usize,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(untagged)]
enum ExternalFrameResultWire {
    NeedMore(ExternalFrameNeedMoreWire),
    Complete(ExternalFrameCompleteWire),
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalFrameNeedMoreWire {
    status: ExternalFrameNeedMoreStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalFrameNeedMoreStatus {
    NeedMore,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalFrameCompleteWire {
    status: ExternalFrameCompleteStatus,
    consumed_bytes: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalFrameCompleteStatus {
    Complete,
}

impl TryFrom<ExternalFrameResultWire> for ExternalFrameResult {
    type Error = DomainError;

    fn try_from(value: ExternalFrameResultWire) -> Result<Self, Self::Error> {
        match value {
            ExternalFrameResultWire::NeedMore(_) => Ok(Self::NeedMore),
            ExternalFrameResultWire::Complete(ExternalFrameCompleteWire {
                consumed_bytes, ..
            }) if consumed_bytes > 0 => Ok(Self::Complete { consumed_bytes }),
            ExternalFrameResultWire::Complete(_) => Err(DomainError::new(
                ErrorCode::BodyDecodeFailed,
                "frame complete 的 consumed_bytes 必须大于零",
            )
            .with_field_error("consumed_bytes", "必须大于零")),
        }
    }
}

impl From<ExternalFrameResult> for ExternalFrameResultWire {
    fn from(value: ExternalFrameResult) -> Self {
        match value {
            ExternalFrameResult::NeedMore => Self::NeedMore(ExternalFrameNeedMoreWire {
                status: ExternalFrameNeedMoreStatus::NeedMore,
            }),
            ExternalFrameResult::Complete { consumed_bytes } => {
                Self::Complete(ExternalFrameCompleteWire {
                    status: ExternalFrameCompleteStatus::Complete,
                    consumed_bytes,
                })
            }
        }
    }
}

impl<'de> Deserialize<'de> for ExternalFrameResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        ExternalFrameResultWire::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for ExternalFrameResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ExternalFrameResultWire::from(self.clone()).serialize(serializer)
    }
}

/// `decode` 调用的单帧参数。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalDecodeRequest {
    frame_base64: String,
}

impl ExternalDecodeRequest {
    /// 将单帧原始字节编码为 canonical Base64 参数。
    #[must_use]
    pub fn from_bytes(frame: &[u8]) -> Self {
        Self {
            frame_base64: STANDARD.encode(frame),
        }
    }

    /// 校验并解码单帧字节。
    pub fn bytes(&self) -> Result<Vec<u8>, DomainError> {
        decode_canonical_base64("frame_base64", &self.frame_base64)
    }
}

/// `decode` 调用返回的外部 Document。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalDecodeResponse {
    /// 尚未绑定方向 Schema 的严格 external Document wire。
    pub document: ExternalDocumentWire,
}

/// `encode` 调用接收的 external Document。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalEncodeRequest {
    /// 由共享领域 Document 规范化得到的 wire。
    pub document: ExternalDocumentWire,
}

/// `encode` 调用返回的线路帧。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalEncodeResponse {
    frame_base64: String,
}

impl ExternalEncodeResponse {
    /// 从待发送线路字节构造响应；主要供测试替身和适配器使用。
    #[must_use]
    pub fn from_bytes(frame: &[u8]) -> Self {
        Self {
            frame_base64: STANDARD.encode(frame),
        }
    }

    /// 校验并解码待发送线路帧。
    pub fn bytes(&self) -> Result<Vec<u8>, DomainError> {
        decode_canonical_base64("frame_base64", &self.frame_base64)
    }
}

/// `display` 调用接收的 external Document。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalDisplayRequest {
    /// 由共享领域 Document 规范化得到的 wire。
    pub document: ExternalDocumentWire,
}

/// `display` 调用返回的未清洗 HTML。
///
/// HTML 安全清洗和 128 KiB 限制属于外层应用端口，本领域 DTO 不把展示策略写入合同转换。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct ExternalDisplayResponse {
    /// 第三方生成、尚待外层安全清洗的 HTML。
    pub html: String,
}
