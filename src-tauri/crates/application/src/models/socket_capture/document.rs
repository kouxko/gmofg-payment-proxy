//! Socket capture 专用的无损 Document 投影。
//!
//! 领域 [`DocumentValue::Int`] 是完整 `i64`，而 JavaScript `number` 只能精确表示
//! ±(2^53-1)。正式抓包必须保留原始证据，因此整数在该 DTO 中使用规范十进制字符串。

use std::fmt;

use intercept_proxy_domain::{Document, DocumentFieldType, DocumentSchema, DocumentValue};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Eq, PartialEq, Serialize, Type)]
#[serde(transparent)]
/// 一个规范、可无损往返的 `i64` 十进制文本。
pub struct SocketCaptureInteger(String);

impl SocketCaptureInteger {
    /// 从完整 `i64` 创建规范十进制表示。
    #[must_use]
    pub fn from_i64(value: i64) -> Self {
        Self(value.to_string())
    }

    /// 返回用于 IPC 与持久化的规范十进制文本。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SocketCaptureInteger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let value = text.parse::<i64>().map_err(serde::de::Error::custom)?;
        if value.to_string() != text {
            return Err(serde::de::Error::custom(
                "Socket capture integer must use canonical decimal form",
            ));
        }
        Ok(Self(text))
    }
}

impl fmt::Debug for SocketCaptureInteger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SocketCaptureInteger(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
/// Capture Document 的类型化值；整数使用十进制字符串保持 `i64` 精度。
pub enum SocketCaptureDocumentValue {
    /// UTF-8 文本值。
    String(String),
    /// 使用规范十进制字符串承载的完整 `i64`。
    Int(SocketCaptureInteger),
    /// 布尔值。
    Bool(bool),
    /// 原始字节值。
    Blob(Vec<u8>),
}

impl<'de> Deserialize<'de> for SocketCaptureDocumentValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(
            tag = "type",
            content = "value",
            rename_all = "snake_case",
            deny_unknown_fields
        )]
        enum Wire {
            String(String),
            Int(SocketCaptureInteger),
            Bool(bool),
            Blob(Vec<u8>),
        }

        Ok(match Wire::deserialize(deserializer)? {
            Wire::String(value) => Self::String(value),
            Wire::Int(value) => Self::Int(value),
            Wire::Bool(value) => Self::Bool(value),
            Wire::Blob(value) => Self::Blob(value),
        })
    }
}

impl SocketCaptureDocumentValue {
    const fn field_type(&self) -> DocumentFieldType {
        match self {
            Self::String(_) => DocumentFieldType::String,
            Self::Int(_) => DocumentFieldType::Int,
            Self::Bool(_) => DocumentFieldType::Bool,
            Self::Blob(_) => DocumentFieldType::Blob,
        }
    }
}

impl fmt::Debug for SocketCaptureDocumentValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketCaptureDocumentValue")
            .field("type", &self.field_type())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
/// 捕获时的完整 Schema 与稀疏值槽；值顺序与 Schema 字段顺序严格一致。
pub struct SocketCaptureDocument {
    /// 捕获时冻结的完整 Schema。
    pub schema: DocumentSchema,
    /// 与 Schema 字段顺序一一对应的可选值槽。
    pub values: Vec<Option<SocketCaptureDocumentValue>>,
}

impl SocketCaptureDocument {
    /// 从领域 Document 创建不会丢失 `i64` 精度的只读投影。
    #[must_use]
    pub fn from_document(document: &Document) -> Self {
        let values = document
            .fields()
            .map(|state| state.value.map(capture_value))
            .collect();
        Self {
            schema: document.schema().clone(),
            values,
        }
    }

    /// 按 Schema 字段名读取已赋值字段。
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SocketCaptureDocumentValue> {
        self.schema
            .fields()
            .iter()
            .position(|field| field.name().as_str() == name)
            .and_then(|index| self.values.get(index))
            .and_then(Option::as_ref)
    }

    pub(super) fn is_consistent_with_schema(&self, expected_id: &str, version: u32) -> bool {
        self.schema.id().as_str() == expected_id
            && self.schema.version() == version
            && self.values.len() == self.schema.fields().len()
            && self
                .schema
                .fields()
                .iter()
                .zip(&self.values)
                .all(|(field, value)| {
                    value
                        .as_ref()
                        .is_none_or(|value| value.field_type() == field.field_type())
                })
    }

    pub(super) fn logical_bytes(&self) -> u64 {
        serde_json::to_vec(self).map_or(u64::MAX, |bytes| bytes.len() as u64)
    }
}

impl fmt::Debug for SocketCaptureDocument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SocketCaptureDocument")
            .field("schema_id", self.schema.id())
            .field("schema_version", &self.schema.version())
            .field(
                "assigned_fields",
                &self.values.iter().filter(|value| value.is_some()).count(),
            )
            .finish()
    }
}

fn capture_value(value: &DocumentValue) -> SocketCaptureDocumentValue {
    match value {
        DocumentValue::String(value) => SocketCaptureDocumentValue::String(value.clone()),
        DocumentValue::Int(value) => {
            SocketCaptureDocumentValue::Int(SocketCaptureInteger::from_i64(*value))
        }
        DocumentValue::Bool(value) => SocketCaptureDocumentValue::Bool(*value),
        DocumentValue::Blob(value) => SocketCaptureDocumentValue::Blob(value.clone()),
    }
}
