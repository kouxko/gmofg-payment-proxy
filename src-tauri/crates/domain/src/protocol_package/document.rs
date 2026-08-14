use super::{DocumentField, DocumentFieldType, DocumentSchema};
use crate::{DomainError, ErrorCode};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
/// 当前 Frame 中某个已声明字段的实际值。
///
/// serde 使用相邻标签 `{ "type": ..., "value": ... }`，因此文本 `"7"`、整数 `7` 和
/// 单字节 Blob `[7]` 不会产生歧义或隐式转换。
pub enum DocumentValue {
    /// 对应 [`DocumentFieldType::String`]。
    String(String),
    /// 对应 [`DocumentFieldType::Int`]。
    Int(i64),
    /// 对应 [`DocumentFieldType::Bool`]。
    Bool(bool),
    /// 对应 [`DocumentFieldType::Blob`]。
    Blob(Vec<u8>),
}

impl DocumentValue {
    #[must_use]
    /// 返回值自身的 Schema 类型，用于写入和反序列化校验。
    pub const fn field_type(&self) -> DocumentFieldType {
        match self {
            Self::String(_) => DocumentFieldType::String,
            Self::Int(_) => DocumentFieldType::Int,
            Self::Bool(_) => DocumentFieldType::Bool,
            Self::Blob(_) => DocumentFieldType::Blob,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// [`Document::fields`] 返回的借用视图。
///
/// `value = None` 表示字段已在 Schema 声明，但当前报文没有为它赋值；它不是未知字段。
pub struct DocumentFieldState<'a> {
    /// 按 Schema 顺序借用的字段声明。
    pub field: &'a DocumentField,
    /// 当前 Frame 的字段值；未赋值时为 `None`。
    pub value: Option<&'a DocumentValue>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type)]
#[serde(try_from = "DocumentWire", into = "DocumentWire")]
/// 一个完整 Frame 解码后的稀疏、有序字段集合。
///
/// Document 共享不可变 [`DocumentSchema`]，并为每个 Schema 字段维护一个同索引的可空值槽。
/// 这种表示不需要第三方有序 Map，也天然区分“字段未声明”和“已声明但当前未赋值”。
pub struct Document {
    schema: Arc<DocumentSchema>,
    values: Vec<Option<DocumentValue>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
// Arc 不直接进入 wire；反序列化后重新共享 Schema，并校验槽数量及每个非空值的类型。
struct DocumentWire {
    schema: DocumentSchema,
    values: Vec<Option<DocumentValue>>,
}

impl Document {
    #[must_use]
    /// 为指定 Schema 创建所有字段均未赋值的 Document。
    ///
    /// 接受 `DocumentSchema` 或 `Arc<DocumentSchema>`；后续 Clone 只增加 Schema 引用计数。
    pub fn new(schema: impl Into<Arc<DocumentSchema>>) -> Self {
        let schema = schema.into();
        let values = vec![None; schema.fields().len()];
        Self { schema, values }
    }

    #[must_use]
    /// 返回当前 Document 绑定的不可变 Schema。
    pub fn schema(&self) -> &DocumentSchema {
        &self.schema
    }

    /// 写入或替换一个已声明字段的值。
    ///
    /// 未知字段返回 [`ErrorCode::DocumentFieldUndeclared`]；值类型与 Schema 不符返回
    /// [`ErrorCode::DocumentFieldTypeMismatch`]，失败时原值槽保持不变。
    pub fn set(&mut self, name: &str, value: DocumentValue) -> Result<(), DomainError> {
        let index = self.field_index(name)?;
        let expected = self.schema.fields()[index].field_type();
        let actual = value.field_type();
        if expected != actual {
            return Err(DomainError::new(
                ErrorCode::DocumentFieldTypeMismatch,
                format!(
                    "字段 {name} 需要 {}，实际得到 {}",
                    expected.as_str(),
                    actual.as_str()
                ),
            )
            .with_field_error(
                format!("document.{name}"),
                format!("需要 {}，实际得到 {}", expected.as_str(), actual.as_str()),
            ));
        }
        self.values[index] = Some(value);
        Ok(())
    }

    /// 读取一个已赋值字段。
    ///
    /// 未知字段与已声明但未赋值分别返回 `DocumentFieldUndeclared` 和
    /// [`ErrorCode::DocumentFieldUnassigned`]，调用方可以据此给脚本稳定诊断。
    pub fn get(&self, name: &str) -> Result<&DocumentValue, DomainError> {
        let index = self.field_index(name)?;
        self.values[index].as_ref().ok_or_else(|| {
            DomainError::new(
                ErrorCode::DocumentFieldUnassigned,
                format!("字段 {name} 已声明但尚未赋值"),
            )
            .with_field_error(format!("document.{name}"), "字段尚未赋值")
        })
    }

    /// 判断一个已声明字段在当前 Frame 中是否已有值。
    ///
    /// 该方法只表达 presence，不表达协议必填性；未知字段仍然返回错误而不是 `false`。
    pub fn has(&self, name: &str) -> Result<bool, DomainError> {
        let index = self.field_index(name)?;
        Ok(self.values[index].is_some())
    }

    /// 按 Schema 声明顺序遍历所有字段及其可选值。
    ///
    /// 迭代器长度恒等于 `schema().fields().len()`，适合 UI Display 和稳定序列化。
    pub fn fields(&self) -> impl ExactSizeIterator<Item = DocumentFieldState<'_>> {
        self.schema
            .fields()
            .iter()
            .zip(&self.values)
            .map(|(field, value)| DocumentFieldState {
                field,
                value: value.as_ref(),
            })
    }

    fn field_index(&self, name: &str) -> Result<usize, DomainError> {
        self.schema.field_index(name).ok_or_else(|| {
            DomainError::new(
                ErrorCode::DocumentFieldUndeclared,
                format!("字段 {name} 未在 Schema 中声明"),
            )
            .with_field_error(format!("document.{name}"), "字段未声明")
        })
    }

    fn try_from_parts(
        schema: DocumentSchema,
        values: Vec<Option<DocumentValue>>,
    ) -> Result<Self, DomainError> {
        if schema.fields().len() != values.len() {
            return Err(DomainError::new(
                ErrorCode::DocumentSchemaInvalid,
                "Document 值槽数量必须与 Schema 字段数量一致",
            )
            .with_field_error("document.values", "值槽数量与 Schema 字段数量不一致"));
        }

        let document = Self {
            schema: Arc::new(schema),
            values,
        };
        for state in document.fields() {
            if let Some(value) = state.value {
                let expected = state.field.field_type();
                let actual = value.field_type();
                if expected != actual {
                    let name = state.field.name().as_str();
                    return Err(DomainError::new(
                        ErrorCode::DocumentFieldTypeMismatch,
                        format!(
                            "字段 {name} 需要 {}，实际得到 {}",
                            expected.as_str(),
                            actual.as_str()
                        ),
                    )
                    .with_field_error(
                        format!("document.{name}"),
                        format!("需要 {}，实际得到 {}", expected.as_str(), actual.as_str()),
                    ));
                }
            }
        }
        Ok(document)
    }
}

impl TryFrom<DocumentWire> for Document {
    type Error = DomainError;

    fn try_from(value: DocumentWire) -> Result<Self, Self::Error> {
        Self::try_from_parts(value.schema, value.values)
    }
}

impl From<Document> for DocumentWire {
    fn from(value: Document) -> Self {
        Self {
            schema: (*value.schema).clone(),
            values: value.values,
        }
    }
}
