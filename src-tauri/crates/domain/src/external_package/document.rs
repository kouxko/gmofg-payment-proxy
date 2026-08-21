use crate::{Document, DocumentSchema, DocumentValue, DomainError, ErrorCode};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::BTreeMap, fmt};

fn invalid_value(field: &str, message: impl Into<String>) -> DomainError {
    DomainError::new(
        ErrorCode::DocumentFieldTypeMismatch,
        "外部 Document 字段值无效",
    )
    .with_field_error(format!("document.{field}"), message)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(untagged)]
enum ExternalDocumentValueWire {
    String(ExternalStringValueWire),
    Int(ExternalIntValueWire),
    Bool(ExternalBoolValueWire),
    Blob(ExternalBlobValueWire),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalStringValueWire {
    #[serde(rename = "type")]
    kind: ExternalStringKind,
    value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalStringKind {
    String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalIntValueWire {
    #[serde(rename = "type")]
    kind: ExternalIntKind,
    value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalIntKind {
    Int,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalBoolValueWire {
    #[serde(rename = "type")]
    kind: ExternalBoolKind,
    value: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalBoolKind {
    Bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(deny_unknown_fields)]
struct ExternalBlobValueWire {
    #[serde(rename = "type")]
    kind: ExternalBlobKind,
    value_base64: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
enum ExternalBlobKind {
    Blob,
}

/// 外部 JSON-RPC 边界专用的 Document wire。
///
/// Wire 对象的键是 Schema 字段名，缺失键表示“未设置”。值使用 closed tagged union：整数是
/// `i64` 十进制字符串，Blob 是带标准填充的 canonical Base64。调用 [`Self::into_document`]
/// 时才绑定具体方向 Schema，并复用 [`Document::set`] 执行未知字段和类型校验。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct ExternalDocumentWire(BTreeMap<String, ExternalDocumentValueWire>);

impl<'de> Deserialize<'de> for ExternalDocumentWire {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ExternalDocumentVisitor;

        impl<'de> Visitor<'de> for ExternalDocumentVisitor {
            type Value = ExternalDocumentWire;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("字段名不重复的 external Document 对象")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut values = BTreeMap::new();
                while let Some((name, value)) =
                    map.next_entry::<String, ExternalDocumentValueWire>()?
                {
                    if values.insert(name.clone(), value).is_some() {
                        return Err(serde::de::Error::custom(format!(
                            "external Document 字段 {name} 重复"
                        )));
                    }
                }
                Ok(ExternalDocumentWire(values))
            }
        }

        deserializer.deserialize_map(ExternalDocumentVisitor)
    }
}

impl ExternalDocumentWire {
    /// 将外部值绑定到指定 Schema，并转换为共享领域 [`Document`]。
    pub fn into_document(self, schema: &DocumentSchema) -> Result<Document, DomainError> {
        let mut document = Document::new(schema.clone());
        for (name, value) in self.0 {
            document.set(&name, value.into_document_value(&name)?)?;
        }
        Ok(document)
    }

    /// 从共享领域 Document 构造规范 external wire；未设置字段不会出现在结果中。
    #[must_use]
    pub fn from_document(document: &Document) -> Self {
        let values = document
            .fields()
            .filter_map(|state| {
                state.value.map(|value| {
                    (
                        state.field.name().as_str().to_owned(),
                        ExternalDocumentValueWire::from(value),
                    )
                })
            })
            .collect();
        Self(values)
    }
}

impl ExternalDocumentValueWire {
    fn into_document_value(self, field: &str) -> Result<DocumentValue, DomainError> {
        match self {
            Self::String(value) => Ok(DocumentValue::String(value.value)),
            Self::Int(value) => {
                let parsed = value
                    .value
                    .parse::<i64>()
                    .map_err(|_| invalid_value(field, "int 必须是 i64 范围内的规范十进制字符串"))?;
                if parsed.to_string() != value.value {
                    return Err(invalid_value(
                        field,
                        "int 必须是 i64 范围内的规范十进制字符串",
                    ));
                }
                Ok(DocumentValue::Int(parsed))
            }
            Self::Bool(value) => Ok(DocumentValue::Bool(value.value)),
            Self::Blob(value) => {
                let decoded = STANDARD
                    .decode(value.value_base64.as_bytes())
                    .map_err(|_| {
                        invalid_value(field, "blob 必须是带标准填充的 canonical Base64")
                    })?;
                if STANDARD.encode(&decoded) != value.value_base64 {
                    return Err(invalid_value(
                        field,
                        "blob 必须是带标准填充的 canonical Base64",
                    ));
                }
                Ok(DocumentValue::Blob(decoded))
            }
        }
    }
}

impl From<&DocumentValue> for ExternalDocumentValueWire {
    fn from(value: &DocumentValue) -> Self {
        match value {
            DocumentValue::String(value) => Self::String(ExternalStringValueWire {
                kind: ExternalStringKind::String,
                value: value.clone(),
            }),
            DocumentValue::Int(value) => Self::Int(ExternalIntValueWire {
                kind: ExternalIntKind::Int,
                value: value.to_string(),
            }),
            DocumentValue::Bool(value) => Self::Bool(ExternalBoolValueWire {
                kind: ExternalBoolKind::Bool,
                value: *value,
            }),
            DocumentValue::Blob(value) => Self::Blob(ExternalBlobValueWire {
                kind: ExternalBlobKind::Blob,
                value_base64: STANDARD.encode(value),
            }),
        }
    }
}
